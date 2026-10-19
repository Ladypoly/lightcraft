//! Background automatic keywords; one bounded image job, model and result at a time.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    pub model: String,
    pub minimum: f32,
    pub detector: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: false, model: "tinyclip-39m".into(), minimum: 0.22, detector: None }
    }
}

impl Settings {
    pub(crate) fn checked(mut self) -> Self {
        let Some(model) = lightcraft_autotag::model(&self.model) else { return Self::default() };
        if !self.minimum.is_finite() || !(0.18..=0.5).contains(&self.minimum) {
            self.minimum = model.minimum;
        }
        if self.detector.as_deref().is_some_and(|id| lightcraft_autotag::detector(id).is_none()) {
            self.detector = None;
        }
        self
    }
}

#[derive(Default)]
pub struct State {
    pub settings: Settings,
    pub dir: Option<std::path::PathBuf>,
    pub downloads: crate::face_download::Downloads,
    pub(crate) coverage: Option<(u64, String, serde_json::Value)>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) scan: Scan,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use crate::activity::TaskGuard;
    use lightcraft_catalog::PhotoId;
    use std::collections::{BTreeMap, HashSet, VecDeque};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    };

    #[derive(Default)]
    pub(crate) struct Scan {
        pub manual: bool,
        pub worker: Option<Worker>,
        pub queue: VecDeque<PhotoId>,
        pub stamp: Option<u64>,
        pub view: String,
        pub targets: HashSet<PhotoId>,
        pub busy: bool,
        pub failed: BTreeMap<PhotoId, (String, String)>,
        pub error: Option<String>,
        pub task: Option<TaskGuard>,
        pub peak: usize,
        pub model: String,
    }

    pub(crate) struct Work {
        pub id: PhotoId,
        pub epoch: u64,
        pub content: String,
        pub job: crate::media::RenderJob,
    }

    pub(crate) struct Done {
        pub id: PhotoId,
        pub epoch: u64,
        pub content: String,
        pub scores: Result<Vec<f32>, String>,
        pub objects: Vec<(String, f32)>,
        pub model_error: bool,
    }

    pub(crate) struct Worker {
        pub send: SyncSender<Work>,
        pub receive: Receiver<Done>,
        cancel: Arc<AtomicBool>,
    }

    impl Worker {
        pub fn start(
            dir: std::path::PathBuf,
            model: &'static lightcraft_autotag::Model,
            detector: Option<&'static lightcraft_autotag::Model>,
        ) -> Result<Self, String> {
            let (send, work) = mpsc::sync_channel::<Work>(1);
            let (done, receive) = mpsc::sync_channel(1);
            let cancel = Arc::new(AtomicBool::new(false));
            let stop = cancel.clone();
            std::thread::Builder::new()
                .name("automatic-keywords".into())
                .spawn(move || {
                    let mut tagger = None;
                    let mut object_model = None;
                    while let Ok(work) = work.recv() {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let mut model_error = false;
                        let mut objects = Vec::new();
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            if tagger.is_none() {
                                match lightcraft_autotag::Tagger::load(&dir, model) {
                                    Ok(model) => tagger = Some(model),
                                    Err(e) => {
                                        model_error = true;
                                        return Err(e);
                                    }
                                }
                            }
                            if object_model.is_none()
                                && let Some(detector) = detector
                            {
                                match lightcraft_autotag::Detector::load(&dir, detector) {
                                    Ok(model) => object_model = Some(model),
                                    Err(e) => {
                                        model_error = true;
                                        return Err(e);
                                    }
                                }
                            }
                            if stop.load(Ordering::Relaxed) {
                                return Err("cancelled".into());
                            }
                            let rendered = crate::memory::in_background(|| work.job.run()).rendered?;
                            let image = rendered.image;
                            let Some(model) = tagger.as_ref() else { return Err("tagging model unavailable".into()) };
                            let pixels = image.as_bytes();
                            if let Some(detector) = &object_model {
                                objects = detector.tags(&pixels, image.width as u32, image.height as u32)?;
                            }
                            if stop.load(Ordering::Relaxed) {
                                return Err("cancelled".into());
                            }
                            model.scores(&pixels, image.width as u32, image.height as u32)
                        }));
                        let scores = match result {
                            Ok(scores) => scores,
                            Err(_) => {
                                model_error = true;
                                Err("automatic keyword worker stopped unexpectedly".into())
                            }
                        };
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        if done.send(Done { id: work.id, epoch: work.epoch, content: work.content, scores, objects, model_error }).is_err() {
                            break;
                        }
                    }
                })
                .map_err(|e| format!("could not start automatic keywords: {e}"))?;
            Ok(Self { send, receive, cancel })
        }
    }

    impl Drop for Worker {
        fn drop(&mut self) {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }
}
