use std::io::Read;
use std::path::Path;

use candle_core::{DType, Device, Module, Tensor};
use candle_nn::{LayerNorm, Linear, VarBuilder};
use image::GenericImageView;
use image::imageops::FilterType;
use sha2::{Digest, Sha256};

use crate::{Model, vocabulary};

type Result<T> = std::result::Result<T, String>;

// Fixed, pinned model shapes. Never allocate a network from untrusted model configuration.
struct Layer {
    norm1: LayerNorm,
    norm2: LayerNorm,
    q: Linear,
    k: Linear,
    v: Linear,
    out: Linear,
    fc1: Linear,
    fc2: Linear,
    heads: usize,
}

impl Layer {
    fn new(vb: VarBuilder, width: usize, heads: usize) -> candle_core::Result<Self> {
        let att = vb.pp("self_attn");
        Ok(Self {
            norm1: candle_nn::layer_norm(width, 1e-5, vb.pp("layer_norm1"))?,
            norm2: candle_nn::layer_norm(width, 1e-5, vb.pp("layer_norm2"))?,
            q: candle_nn::linear(width, width, att.pp("q_proj"))?,
            k: candle_nn::linear(width, width, att.pp("k_proj"))?,
            v: candle_nn::linear(width, width, att.pp("v_proj"))?,
            out: candle_nn::linear(width, width, att.pp("out_proj"))?,
            fc1: candle_nn::linear(width, width * 4, vb.pp("mlp.fc1"))?,
            fc2: candle_nn::linear(width * 4, width, vb.pp("mlp.fc2"))?,
            heads,
        })
    }

    fn forward(&self, x: &Tensor, causal: Option<&Tensor>) -> candle_core::Result<Tensor> {
        let (b, n, width) = x.dims3()?;
        let h = self.norm1.forward(x)?;
        let split = |v: Tensor| v.reshape((b, n, self.heads, width / self.heads))?.transpose(1, 2)?.contiguous();
        let q = split(self.q.forward(&h)?)?;
        let k = split(self.k.forward(&h)?)?;
        let v = split(self.v.forward(&h)?)?;
        let mut att = (q.matmul(&k.t()?)? * (width as f64 / self.heads as f64).powf(-0.5))?;
        if let Some(bias) = causal {
            att = att.broadcast_add(bias)?;
        }
        let att = candle_nn::ops::softmax_last_dim(&att)?.matmul(&v)?.transpose(1, 2)?.reshape((b, n, width))?;
        let x = x.add(&self.out.forward(&att)?)?;
        let h = self.fc1.forward(&self.norm2.forward(&x)?)?.gelu_erf()?;
        x.add(&self.fc2.forward(&h)?)
    }
}

fn layers(vb: VarBuilder, n: usize, width: usize, heads: usize) -> candle_core::Result<Vec<Layer>> {
    (0..n).map(|i| Layer::new(vb.pp(format!("encoder.layers.{i}")), width, heads)).collect()
}

fn normalized(x: &Tensor) -> candle_core::Result<Tensor> {
    x.broadcast_div(&x.sqr()?.sum_keepdim(candle_core::D::Minus1)?.sqrt()?)
}

/// One model and its fixed vocabulary embeddings. Held by the single background worker.
pub struct Tagger {
    patch: candle_nn::Conv2d,
    class: Tensor,
    positions: Tensor,
    pre: LayerNorm,
    post: LayerNorm,
    projection: Linear,
    layers: Vec<Layer>,
    labels: Tensor,
    width: usize,
}

impl Tagger {
    /// Verify both size and digest before interpreting a model. Safe buffered tensors, never mmap/unsafe.
    pub fn load(dir: &Path, model: &Model) -> Result<Self> {
        let bytes = read_model(dir, model)?;
        Self::build(bytes, model).map_err(|e| format!("cannot load tagging model: {e}"))
    }

    fn build(bytes: Vec<u8>, model: &Model) -> candle_core::Result<Self> {
        let device = Device::Cpu;
        let vb = VarBuilder::from_buffered_safetensors(bytes, DType::F32, &device)?;
        let (width, heads, depth, text_depth) = if model.id == "tinyclip-8m" { (256, 4, 10, 3) } else { (512, 8, 12, 6) };
        let vision = vb.pp("vision_model");
        let e = vision.pp("embeddings");
        let patch = candle_nn::conv2d_no_bias(3, width, 16, candle_nn::Conv2dConfig { stride: 16, ..Default::default() }, e.pp("patch_embedding"))?;
        let class = e.get(width, "class_embedding")?.reshape((1, 1, width))?;
        let positions = e.get((197, width), "position_embedding.weight")?.unsqueeze(0)?;
        let pre = candle_nn::layer_norm(width, 1e-5, vision.pp("pre_layrnorm"))?;
        let post = candle_nn::layer_norm(width, 1e-5, vision.pp("post_layernorm"))?;
        let projection = candle_nn::linear_no_bias(width, 512, vb.pp("visual_projection"))?;
        let image_layers = layers(vision, depth, width, heads)?;
        let text = vb.pp("text_model");
        let tokens = candle_nn::embedding(49408, width, text.pp("embeddings.token_embedding"))?;
        let positions_text = text.pp("embeddings").get((77, width), "position_embedding.weight")?.unsqueeze(0)?;
        let text_layers = layers(text.clone(), text_depth, width, heads)?;
        let final_norm = candle_nn::layer_norm(width, 1e-5, text.pp("final_layer_norm"))?;
        let text_proj = candle_nn::linear_no_bias(width, 512, vb.pp("text_projection"))?;
        let mut bias = vec![0f32; 77 * 77];
        for (q, row) in bias.chunks_mut(77).enumerate() {
            for (k, v) in row.iter_mut().enumerate() {
                if k > q {
                    *v = f32::MIN;
                }
            }
        }
        let bias = Tensor::from_vec(bias, (1, 1, 77, 77), &device)?;
        let mut features = Vec::new();
        for (_, mut ids) in vocabulary() {
            let end = ids.len().saturating_sub(1);
            ids.resize(77, 49407);
            let ids = Tensor::from_vec(ids, (1, 77), &device)?;
            let mut x = tokens.forward(&ids)?.broadcast_add(&positions_text)?;
            for l in &text_layers {
                x = l.forward(&x, Some(&bias))?;
            }
            let x = final_norm.forward(&x)?.narrow(1, end, 1)?.squeeze(1)?;
            features.push(normalized(&text_proj.forward(&x)?)?);
        }
        let labels = Tensor::cat(&features, 0)?.t()?.contiguous()?;
        Ok(Self { patch, class, positions, pre, post, projection, layers: image_layers, labels, width })
    }

    /// Combine the pinned centre view with the full frame so edge subjects remain visible.
    pub fn scores(&self, rgba: &[u8], width: u32, height: u32) -> Result<Vec<f32>> {
        let input = preprocess(rgba, width, height)?;
        let mut scores = self.forward(input).map_err(|e| format!("tagging inference: {e}"))?;
        if width != height {
            let image = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or("invalid tagging preview pixels")?;
            let full = image::imageops::resize(&image, 224, 224, FilterType::CatmullRom);
            let full = self.forward(preprocess(full.as_raw(), 224, 224)?).map_err(|e| format!("tagging inference: {e}"))?;
            for (score, full) in scores.iter_mut().zip(full) {
                *score = score.max(full);
            }
        }
        Ok(scores)
    }

    fn forward(&self, input: Vec<f32>) -> candle_core::Result<Vec<f32>> {
        let x = Tensor::from_vec(input, (1, 3, 224, 224), &Device::Cpu)?;
        let patches = self.patch.forward(&x)?.flatten_from(2)?.transpose(1, 2)?;
        let mut x = Tensor::cat(&[&self.class, &patches], 1)?.broadcast_add(&self.positions)?;
        x = self.pre.forward(&x)?;
        for l in &self.layers {
            x = l.forward(&x, None)?;
        }
        let x = x.narrow(1, 0, 1)?.reshape((1, self.width))?;
        let x = normalized(&self.projection.forward(&self.post.forward(&x)?)?)?;
        x.matmul(&self.labels)?.squeeze(0)?.to_vec1()
    }
}

fn preprocess(rgba: &[u8], width: u32, height: u32) -> Result<Vec<f32>> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 || u64::from(width) * u64::from(height) > 16_777_216 {
        return Err("tagging preview dimensions are invalid".into());
    }
    let image = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or("invalid tagging preview pixels")?;
    // Bound very thin panoramas before resizing; they must not allocate gigapixel intermediates.
    let scale = 224.0 / width.min(height) as f64;
    let w = (width as f64 * scale).floor().clamp(224.0, 4096.0) as u32;
    let h = (height as f64 * scale).floor().clamp(224.0, 4096.0) as u32;
    let resized = image::imageops::resize(&image, w, h, FilterType::CatmullRom);
    let cropped = image::imageops::crop_imm(&resized, (w - 224) / 2, (h - 224) / 2, 224, 224);
    let mut out = vec![0f32; 3 * 224 * 224];
    for (i, pixel) in cropped.pixels().enumerate() {
        let (_, _, p) = pixel;
        for (channel, (mean, std)) in [(0.48145466, 0.26862954), (0.4578275, 0.26130258), (0.40821073, 0.27577711)].into_iter().enumerate() {
            if let (Some(dst), Some(value)) = (out.get_mut(channel * 224 * 224 + i), p.0.get(channel)) {
                *dst = (*value as f32 / 255.0 - mean) / std;
            }
        }
    }
    Ok(out)
}

/// Bounded read shared by the pinned CLIP and YOLOX models.
pub(crate) fn read_model(dir: &Path, model: &Model) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(dir.join(model.id).join(model.file())).map_err(|e| format!("model: {e}"))?;
    if file.metadata().map_err(|e| e.to_string())?.len() != model.bytes {
        return Err("model size does not match; download it again".into());
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(model.bytes as usize).map_err(|e| e.to_string())?;
    file.by_ref().take(model.bytes + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != model.bytes || format!("{:x}", Sha256::digest(&bytes)) != model.sha256 {
        return Err("model checksum does not match; download it again".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preprocessing_is_bounded_and_normalized() {
        assert!(preprocess(&[], 0, 1).is_err());
        assert!(preprocess(&[], u32::MAX, 1).is_err());
        assert!(preprocess(&[], 32, 32).is_err());
        let data = vec![255; 224 * 224 * 4];
        let x = preprocess(&data, 224, 224).unwrap();
        assert_eq!(x.len(), 3 * 224 * 224);
        assert!((x[0] - (1.0 - 0.48145466) / 0.26862954).abs() < 1e-5);
    }
    #[test]
    fn installed_models_run_real_inference() {
        let Some(dir) = std::env::var_os("LIGHTCRAFT_TAG_MODELS") else {
            lightcraft_testkit::skip!(Model, "set LIGHTCRAFT_TAG_MODELS to run both pinned TinyCLIP models");
            return;
        };
        for model in crate::MODELS {
            let start = std::time::Instant::now();
            let tagger = Tagger::load(Path::new(&dir), model).unwrap();
            let loaded = start.elapsed();
            let scores = tagger.scores(&vec![128; 224 * 224 * 4], 224, 224).unwrap();
            assert_eq!(scores.len(), vocabulary().count());
            assert!(scores.iter().all(|s| s.is_finite() && s.abs() <= 1.001));
            eprintln!("{}: load {loaded:?}, first inference {:?}", model.id, start.elapsed() - loaded);
            let mut pixels = Vec::new();
            for y in 0..224 {
                for x in 0..224 {
                    pixels.extend_from_slice(&[((x * 3 + y) % 256) as u8, ((y * 2) % 256) as u8, ((x + y * 4) % 256) as u8, 255]);
                }
            }
            let expected: std::collections::BTreeMap<String, Vec<f32>> = serde_json::from_str(include_str!("reference-scores.json")).unwrap();
            let actual = tagger.scores(&pixels, 224, 224).unwrap();
            let reference = &expected[model.id];
            assert_eq!(actual.len(), reference.len());
            let error = actual.iter().zip(reference).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
            assert!(error < 0.0002, "{} differs from Transformers reference by {error}", model.id);
            let mut wide_pixels = Vec::new();
            for row in pixels.as_chunks::<{ 224 * 4 }>().0 {
                wide_pixels.extend_from_slice(row);
                wide_pixels.extend_from_slice(row);
            }
            let wide = tagger.scores(&wide_pixels, 448, 224).unwrap();
            let error = wide.iter().zip(&expected[&format!("{}-wide", model.id)]).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
            assert!(error < 0.0002, "{} wide-frame reference error {error}", model.id);
            let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
            for (file, required, absent) in [
                ("cat-chair.jpg", &["cat", "chair"][..], &[][..]),
                ("cat-chair-room.jpg", &["cat", "chair"][..], &[][..]),
                ("cat-only.jpg", &["cat"][..], &["chair"][..]),
                ("chair-only.jpg", &["chair"][..], &["cat"][..]),
                ("coffee-cup.jpg", &["coffee", "cup"][..], &["cat", "chair"][..]),
                ("camera-person-outdoors.jpg", &["person", "camera", "outdoors"][..], &["cat", "chair"][..]),
            ] {
                let image = image::open(fixtures.join(file)).unwrap().into_rgba8();
                let scores = tagger.scores(image.as_raw(), image.width(), image.height()).unwrap();
                let tags: Vec<_> = crate::select(&scores, model.minimum).into_iter().map(|(name, _)| name).collect();
                eprintln!("{} {file}: {tags:?}", model.id);
                for required in required {
                    assert!(tags.iter().any(|t| t == required), "{} {file} missing {required}: {tags:?}", model.id);
                }
                for absent in absent {
                    assert!(!tags.iter().any(|t| t == absent), "{} {file} false {absent}: {tags:?}", model.id);
                }
            }
            if let Some(images) = std::env::var_os("LIGHTCRAFT_TAG_TEST_IMAGES") {
                for (file, expected) in [("chelsea.png", "cat"), ("coffee.png", "coffee"), ("horse.png", "horse"), ("camera.png", "camera")] {
                    let image = image::open(Path::new(&images).join(file)).unwrap().into_rgba8();
                    let scores = tagger.scores(image.as_raw(), image.width(), image.height()).unwrap();
                    let tags = crate::select(&scores, model.minimum);
                    assert!(tags.iter().any(|(name, _)| name == expected), "{} {file}: {tags:?}", model.id);
                    eprintln!("{} {file}: {tags:?}", model.id);
                }
            }
        }
    }
}
