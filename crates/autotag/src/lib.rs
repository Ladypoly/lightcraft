//! Opt-in local image tagging. No network access, filesystem changes or inference during search.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde::Serialize;

#[cfg(not(target_arch = "wasm32"))]
mod runtime;
#[cfg(not(target_arch = "wasm32"))]
pub use runtime::Tagger;
#[cfg(not(target_arch = "wasm32"))]
mod detector;
#[cfg(not(target_arch = "wasm32"))]
pub use detector::Detector;

/// Changing labels, templates, preprocessing or thresholds requires a new index version.
pub const INDEX_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub licence: &'static str,
    pub minimum: f32,
}

pub const MODELS: &[Model] = &[
    Model {
        id: "tinyclip-8m",
        name: "TinyCLIP Compact",
        description: "Smallest download and lower CPU/memory cost. Windows, macOS and Linux; CPU.",
        repo: "wkcn/TinyCLIP-ViT-8M-16-Text-3M-YFCC15M",
        revision: "a2a8c6eaa2549ad66eb7c31b85022bf58273a26c",
        bytes: 93_812_468,
        sha256: "9339ee3d736344d0ddcaa6c03edc9f89688f08caaea5401220885233da726fcc",
        licence: "MIT",
        minimum: 0.34,
    },
    Model {
        id: "tinyclip-39m",
        name: "TinyCLIP Balanced",
        description: "Larger model with higher published recognition accuracy. More CPU/memory. Windows, macOS and Linux; CPU.",
        repo: "wkcn/TinyCLIP-ViT-39M-16-Text-19M-YFCC15M",
        revision: "07a4b0bc751cb64fecd2b661c048c1dd98d69444",
        bytes: 332_443_148,
        sha256: "112bdfb07ebc8994e9afba7a2a314e278493a675033595cf48e54cf8a17ae587",
        licence: "MIT",
        minimum: 0.22,
    },
];

pub const DETECTORS: &[Model] = &[
    Model {
        id: "yolox-nano",
        name: "YOLOX Nano",
        description: "Fast object detection. 80 COCO object classes; 416 px; CPU.",
        repo: "Megvii-BaseDetection/YOLOX",
        revision: "0.1.1rc0",
        bytes: 3_659_407,
        sha256: "c789161ed43c8269fcd4e67c67eeeb4e80c622da2eb296a20bc6007bd18a0b7d",
        licence: "Apache-2.0",
        minimum: 0.35,
    },
    Model {
        id: "yolox-s",
        name: "YOLOX Small",
        description: "Better object detection, higher CPU cost. 80 COCO object classes; 640 px; CPU.",
        repo: "Megvii-BaseDetection/YOLOX",
        revision: "0.1.1rc0",
        bytes: 35_858_002,
        sha256: "c5c2d13e59ae883e6af3b45daea64af4833a4951c92d116ec270d9ddbe998063",
        licence: "Apache-2.0",
        minimum: 0.35,
    },
];

pub fn detector(id: &str) -> Option<&'static Model> {
    DETECTORS.iter().find(|m| m.id == id)
}

pub fn model(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id == id)
}

impl Model {
    pub fn url(&self) -> String {
        if detector(self.id).is_some() {
            format!("https://github.com/{}/releases/download/{}/{}.onnx", self.repo, self.revision, self.id.replace('-', "_"))
        } else {
            format!("https://huggingface.co/{}/resolve/{}/model.safetensors", self.repo, self.revision)
        }
    }
    pub fn file(&self) -> &'static str {
        if detector(self.id).is_some() { "model.onnx" } else { "model.safetensors" }
    }
    pub fn source(&self) -> String {
        format!("https://{}/{}", if detector(self.id).is_some() { "github.com" } else { "huggingface.co" }, self.repo)
    }
    pub fn key(&self) -> String {
        format!("{}@{}:{INDEX_VERSION}", self.id, self.sha256)
    }
}

/// Independent label evidence, not a softmax distribution or a required number of tags.
pub fn select(scores: &[f32], minimum: f32) -> Vec<(String, f32)> {
    select_context(scores, minimum, false)
}

/// With an object detector, CLIP contributes context only; it cannot add guessed object labels.
pub fn select_context(scores: &[f32], minimum: f32, objects: bool) -> Vec<(String, f32)> {
    if !minimum.is_finite() {
        return Vec::new();
    }
    let mut candidates: Vec<_> = entries()
        .zip(scores)
        .filter(|((_, _, category), score)| score.is_finite() && !(objects && *category == "subject"))
        .map(|((label, _, category), score)| (label, *score, category))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let n = candidates.len() as f64;
    let mean = candidates.iter().map(|(_, s, _)| f64::from(*s)).sum::<f64>() / n;
    let variance = candidates.iter().map(|(_, s, _)| (f64::from(*s) - mean).powi(2)).sum::<f64>() / n;
    // Reject the image's generic background responses without comparing every label to its winner.
    let floor = minimum.max((mean + 2.5 * variance.sqrt()) as f32);
    candidates.retain(|(_, score, _)| *score >= floor);
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
    let mut selected = Vec::new();
    // Reserve space for context so similar object labels cannot crowd out the scene.
    for (category, limit) in [("subject", 6), ("scene", 2), ("activity", 2), ("style", 2)] {
        let mut count = 0;
        let mut location = false;
        for &(label, score, kind) in &candidates {
            if kind != category || (objects && category == "subject") {
                continue;
            }
            let is_location = matches!(label, "indoors" | "outdoors");
            if is_location && location {
                continue;
            }
            selected.push((label.to_string(), score));
            location |= is_location;
            count += 1;
            if count == limit {
                break;
            }
        }
    }
    selected.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    selected
}

fn entries() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    include_str!("vocabulary.tsv").lines().filter_map(|line| {
        let mut fields = line.split('\t');
        Some((fields.next()?, fields.next()?, fields.next()?))
    })
}

pub fn vocabulary() -> impl Iterator<Item = (&'static str, Vec<u32>)> {
    entries().map(|(label, tokens, _)| (label, tokens.split(',').filter_map(|n| n.parse().ok()).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_original_vocabulary_and_scores() {
        let labels: Vec<_> = vocabulary().collect();
        assert!(labels.len() > 100);
        for (label, ids) in &labels {
            assert!(!label.is_empty() && label.is_ascii());
            assert_eq!(ids.first(), Some(&49406));
            assert_eq!(ids.last(), Some(&49407));
            assert!(ids.len() < 77 && ids.iter().all(|i| *i < 49408));
        }
        assert!(select(&vec![0.1; labels.len()], 0.25).is_empty());
        assert!(select(&[f32::NAN, f32::INFINITY, 0.3], 0.25).iter().all(|(_, s)| s.is_finite()));
        assert_eq!(select(&vec![0.3; labels.len()], 0.25).len(), 12);
        assert!(select(&vec![0.3; labels.len()], f32::NAN).is_empty());
        let mut unique = std::collections::HashSet::new();
        for (label, _, category) in entries() {
            assert!(unique.insert(label));
            assert!(["subject", "scene", "style", "activity"].contains(&category));
        }
    }
    #[test]
    fn dominant_subject_does_not_suppress_secondary_objects_or_scene() {
        let mut scores = vec![0.1; vocabulary().count()];
        for (name, score) in [("cat", 0.8), ("chair", 0.3), ("indoors", 0.28), ("office", 0.26), ("portrait", 0.25)] {
            let i = vocabulary().position(|(label, _)| label == name).unwrap();
            scores[i] = score;
        }
        let tags: Vec<_> = select(&scores, 0.22).into_iter().map(|(name, _)| name).collect();
        assert_eq!(tags, ["cat", "chair", "indoors", "office", "portrait"]);
        scores.fill(0.1);
        scores[vocabulary().position(|(label, _)| label == "cat").unwrap()] = 0.8;
        assert_eq!(select(&scores, 0.22).len(), 1, "never invent tags to fill a quota");
    }

    #[test]
    fn scene_survives_many_object_candidates_and_locations_do_not_conflict() {
        let mut scores = vec![0.1; vocabulary().count()];
        for (i, (_, _, category)) in entries().enumerate() {
            if category == "subject" && i < 15 {
                scores[i] = 0.4;
            }
        }
        for (name, score) in [("outdoors", 0.35), ("indoors", 0.34), ("garden", 0.33)] {
            scores[vocabulary().position(|(label, _)| label == name).unwrap()] = score;
        }
        let tags: Vec<_> = select(&scores, 0.22).into_iter().map(|(name, _)| name).collect();
        assert!(tags.contains(&"outdoors".to_string()) && tags.contains(&"garden".to_string()));
        assert!(!tags.contains(&"indoors".to_string()));
        assert_eq!(tags.len(), 8);
    }

    #[test]
    fn only_known_pinned_models() {
        assert!(model("../../other").is_none());
        for m in MODELS {
            assert_eq!(m.sha256.len(), 64);
            assert!(m.bytes < 400_000_000);
        }
    }
}
