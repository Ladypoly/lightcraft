//! CPU-only smoke benchmark on the same six public fixtures; run each model in a fresh process.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#[cfg(not(target_arch = "wasm32"))]
use lightcraft_autotag::{Detector, Tagger};
#[cfg(not(target_arch = "wasm32"))]
use std::{path::PathBuf, time::Instant};

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = std::env::args().nth(1).ok_or("pass a supported CLIP or YOLOX model id")?;
    if id == "--models" {
        let models: Vec<_> = lightcraft_autotag::MODELS
            .iter()
            .chain(lightcraft_autotag::DETECTORS)
            .map(|m| serde_json::json!({"id":m.id,"url":m.url(),"file":m.file(),"bytes":m.bytes,"sha256":m.sha256}))
            .collect();
        println!("{}", serde_json::to_string(&models)?);
        return Ok(());
    }
    let dir = PathBuf::from(std::env::var_os("LIGHTCRAFT_TAG_MODELS").ok_or("set LIGHTCRAFT_TAG_MODELS")?);
    let model = lightcraft_autotag::model(&id).or_else(|| lightcraft_autotag::detector(&id)).ok_or("unknown model")?;
    let started = Instant::now();
    let is_detector = lightcraft_autotag::detector(&id).is_some();
    let tagger = if is_detector { None } else { Some(Tagger::load(&dir, model)?) };
    let detector = if is_detector { Some(Detector::load(&dir, model)?) } else { None };
    let load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut images = Vec::new();
    for name in ["cat-chair.jpg", "cat-chair-room.jpg", "cat-only.jpg", "chair-only.jpg", "coffee-cup.jpg", "camera-person-outdoors.jpg"] {
        images.push((name, image::open(fixtures.join(name))?.into_rgba8()));
    }
    let mut timings = Vec::new();
    let mut tags = Vec::new();
    for round in 0..3 {
        for (name, image) in &images {
            let started = Instant::now();
            let found = if let Some(tagger) = &tagger {
                lightcraft_autotag::select(&tagger.scores(image.as_raw(), image.width(), image.height())?, model.minimum)
            } else if let Some(detector) = &detector {
                detector.tags(image.as_raw(), image.width(), image.height())?
            } else {
                return Err("no model loaded".into());
            };
            timings.push(started.elapsed().as_secs_f64() * 1000.0);
            if round == 0 {
                tags.push(serde_json::json!({"fixture":name,"tags":found}));
            }
        }
    }
    timings.sort_by(f64::total_cmp);
    let median = timings.get(timings.len() / 2).copied().ok_or("no measurements")?;
    println!(
        "{}",
        serde_json::json!({"model":id,"device":"CPU","os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "loadMs":load_ms,"samples":timings.len(),"medianImageMs":median,"meanImageMs":timings.iter().sum::<f64>()/timings.len() as f64,"fixtures":tags})
    );
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {
    eprintln!("This CPU benchmark requires a native build.");
}
