use super::{CommandSpec, always, bad, cmd};
use crate::{Result, Session};
use lightcraft_autotag::{DETECTORS, MODELS, detector, model};
#[cfg(not(target_arch = "wasm32"))]
use lightcraft_catalog::autotags::AutoTag;
use lightcraft_catalog::{Op, PhotoId};
use serde_json::{Value, json};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "tags.models", "Automatic Keyword Models", [], None, "{}", always, models),
        cmd!(
            "tags.configure",
            "Configure Automatic Keywords",
            [],
            None,
            "{enabled?:bool,model?:id,minimum?:0.18..0.5,detector?:id|null}",
            always,
            configure
        ),
        cmd!("tags.scan", "Tag Missing Images", [], None, "{rescan?:bool} (current view; does not enable automatic tagging)", always, scan),
        cmd!("tags.download", "Download Keyword Model", [], None, "{model:id}", always, download),
        cmd!("tags.cancelDownload", "Cancel Keyword Model Download", [], None, "{model:id}", always, cancel_download),
        cmd!(query "tags.pump", "Index Automatic Keywords", [], None, "{pace?:pause|light|normal}", always, pump),
        cmd!(
            "tags.reindex",
            "Reindex Automatic Keywords",
            [],
            None,
            "{ids?:[id]} (omitted: current view, including browsed photos and the view’s filters/subfolder setting)",
            always,
            reindex
        ),
        cmd!(
            "tags.review",
            "Review Automatic Keywords",
            [],
            None,
            "{ids?:[id],names?:[keyword],action:accept|dismiss} (omitted names: all suggestions)",
            always,
            review
        ),
    ]
}

fn requested_model(p: &Value, c: &str) -> Result<&'static lightcraft_autotag::Model> {
    p.get("model").and_then(Value::as_str).and_then(|id| model(id).or_else(|| detector(id))).ok_or_else(|| bad(c, "choose one of tags.models"))
}

fn dir(s: &Session, c: &str) -> Result<std::path::PathBuf> {
    s.tagging.dir.clone().ok_or_else(|| bad(c, "automatic keywords need a native model folder"))
}

fn installed(s: &Session, m: &lightcraft_autotag::Model) -> bool {
    s.tagging.dir.as_ref().and_then(|d| std::fs::metadata(d.join(m.id).join(m.file())).ok()).is_some_and(|f| f.is_file() && f.len() == m.bytes)
}

fn detector_installed(s: &Session, settings: &crate::autotag::Settings) -> bool {
    settings.detector.as_deref().is_none_or(|id| detector(id).is_some_and(|m| installed(s, m)))
}

fn view_key(s: &Session) -> String {
    format!("{:?}|{:?}|{:?}|{:?}", s.source, s.filter, s.browse, s.library_folder)
}

fn model_key(s: &Session) -> String {
    let clip = model(&s.tagging.settings.model).map(|m| format!("{}:{}", m.key(), s.tagging.settings.minimum)).unwrap_or_default();
    match s.tagging.settings.detector.as_deref().and_then(detector) {
        Some(m) => format!("{clip}+{}:{}", m.key(), m.minimum),
        None => clip,
    }
}

fn coverage(s: &mut Session) -> Value {
    let key = model_key(s);
    let stamp = format!("{}|{}|{key}", s.library_generation, view_key(s));
    if let Some((revision, previous, value)) = &s.tagging.coverage
        && *revision == s.catalog.revision
        && *previous == stamp
    {
        return value.clone();
    }
    let (mut total, mut tagged) = (0u64, 0u64);
    for id in s.visible_cloned() {
        let Some(photo) = s.catalog.photo(id) else { continue };
        if photo.deleted || photo.kind == lightcraft_catalog::MediaKind::Video {
            continue;
        }
        total += 1;
        if photo.meta.auto_tags.as_ref().is_some_and(|ai| ai.model == key && ai.content == photo.content_key()) {
            tagged += 1;
        }
    }
    let value = json!({"total":total,"tagged":tagged,"missing":total - tagged});
    s.tagging.coverage = Some((s.catalog.revision, stamp, value.clone()));
    value
}

fn scan(s: &mut Session, p: &Value) -> Result<Value> {
    let rescan = p.get("rescan").map(|v| v.as_bool().ok_or_else(|| bad("tags.scan", "rescan must be true or false"))).transpose()?.unwrap_or(false);
    let m = model(&s.tagging.settings.model).ok_or_else(|| bad("tags.scan", "unknown keyword model"))?;
    if !cfg!(not(target_arch = "wasm32")) || !installed(s, m) || !detector_installed(s, &s.tagging.settings) {
        return Err(bad("tags.scan", "download this model in Settings > Auto Tagging first"));
    }
    if rescan {
        reindex(s, &json!({}))?;
    }
    let count = coverage(s)["missing"].as_u64().unwrap_or(0);
    #[cfg(not(target_arch = "wasm32"))]
    {
        s.tagging.scan = crate::autotag::Scan { manual: count > 0, view: view_key(s), model: model_key(s), ..Default::default() };
    }
    Ok(json!({"queued":count}))
}

fn models(s: &mut Session, _: &Value) -> Result<Value> {
    finish_downloads(s);
    let downloads = s.tagging.downloads.snapshot();
    let rows: Vec<_> = MODELS
        .iter()
        .chain(DETECTORS)
        .map(|m| {
            let mut row = serde_json::to_value(m).unwrap_or(Value::Null);
            row["installed"] = json!(installed(s, m));
            row["source"] = json!(m.source());
            row["download"] = downloads
                .iter()
                .find(|(id, _)| id == m.id)
                .map(|(_, state)| match state {
                    crate::face_download::State::Running { bytes, total } => json!({"state":"running","bytes":bytes,"total":total}),
                    crate::face_download::State::Failed(e) => json!({"state":"failed","error":e}),
                    _ => json!({"state":"ready"}),
                })
                .unwrap_or(Value::Null);
            row
        })
        .collect();
    #[cfg(not(target_arch = "wasm32"))]
    let status = json!({"busy":s.tagging.scan.busy,"pending":s.tagging.scan.queue.len(),"failed":s.tagging.scan.failed.len(),"error":s.tagging.scan.error,
            "failures":s.tagging.scan.failed.iter().take(20).map(|(id,(_,error))|json!({"id":id,"error":error})).collect::<Vec<_>>()});
    #[cfg(target_arch = "wasm32")]
    let status = json!({"error":"Automatic keyword generation is available in native builds. Existing tags remain searchable."});
    let (objects, rows): (Vec<_>, Vec<_>) = rows.into_iter().partition(|row| row["id"].as_str().and_then(detector).is_some());
    let coverage = coverage(s);
    Ok(
        json!({"coverage":coverage,"models":rows,"detectors":objects,"settings":s.tagging.settings,"status":status,"available":cfg!(not(target_arch="wasm32")) && s.tagging.dir.is_some()}),
    )
}

fn configure(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "tags.configure";
    let mut settings = s.tagging.settings.clone();
    if let Some(value) = p.get("model") {
        let m = value.as_str().and_then(model).ok_or_else(|| bad(C, "choose a CLIP model from tags.models"))?;
        if settings.model != m.id {
            settings.minimum = m.minimum;
        }
        settings.model = m.id.into();
    }
    if let Some(value) = p.get("enabled") {
        settings.enabled = value.as_bool().ok_or_else(|| bad(C, "enabled must be true or false"))?;
    }
    if let Some(value) = p.get("minimum") {
        let n = value.as_f64().filter(|n| n.is_finite() && (0.18..=0.5).contains(n)).ok_or_else(|| bad(C, "minimum must be between 0.18 and 0.5"))?;
        settings.minimum = n as f32;
    }
    if let Some(value) = p.get("detector") {
        settings.detector = if value.is_null() {
            None
        } else {
            Some(value.as_str().and_then(detector).ok_or_else(|| bad(C, "choose an object detector from tags.models, or null"))?.id.into())
        };
    }
    let m = model(&settings.model).ok_or_else(|| bad(C, "unknown keyword model"))?;
    if settings.enabled && (!cfg!(not(target_arch = "wasm32")) || !installed(s, m) || !detector_installed(s, &settings)) {
        return Err(bad(C, "download this model before enabling automatic keywords"));
    }
    let old = std::mem::replace(&mut s.tagging.settings, settings);
    if let Err(e) = s.save_prefs() {
        s.tagging.settings = old;
        return Err(e);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        s.tagging.scan = Default::default();
    }
    Ok(json!(s.tagging.settings))
}

fn download(s: &mut Session, p: &Value) -> Result<Value> {
    let m = requested_model(p, "tags.download")?;
    let dir = dir(s, "tags.download")?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (m, dir);
        return Err(bad("tags.download", "model downloads require a native build"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let spec = lightcraft_faces::known::Download {
            id: m.id.into(),
            url: m.url(),
            file_name: format!("{}.{}", m.id, if detector(m.id).is_some() { "onnx" } else { "safetensors" }),
            size_bytes: m.bytes,
            sha256: m.sha256.into(),
        };
        s.tagging.downloads.start(spec, &dir, &s.activity, m.name).map_err(|e| bad("tags.download", e))?;
        Ok(json!({"started":m.id}))
    }
}

fn cancel_download(s: &mut Session, p: &Value) -> Result<Value> {
    let m = requested_model(p, "tags.cancelDownload")?;
    Ok(json!({"cancelled":s.tagging.downloads.discard(m.id)}))
}

fn finish_downloads(s: &mut Session) {
    let Some(root) = &s.tagging.dir else { return };
    for (id, staged, _) in s.tagging.downloads.finished() {
        let Some(m) = model(&id).or_else(|| detector(&id)) else { continue };
        let target = root.join(m.id);
        let result = std::fs::create_dir_all(&target).and_then(|()| std::fs::rename(&staged, target.join(m.file())));
        let state = match result {
            Ok(()) => {
                #[cfg(not(target_arch = "wasm32"))]
                if s.tagging.settings.model == m.id || s.tagging.settings.detector.as_deref() == Some(m.id) {
                    s.tagging.scan = Default::default();
                }
                crate::face_download::State::Installed
            }
            Err(e) => crate::face_download::State::Failed(format!("cannot install model: {e}")),
        };
        s.tagging.downloads.set_outcome(&id, state);
    }
}

fn ids(s: &mut Session, p: &Value, c: &str, view: bool) -> Result<Vec<PhotoId>> {
    let mut ids = if let Some(ids) = p.get("ids") {
        let ids = ids.as_array().filter(|v| !v.is_empty()).ok_or_else(|| bad(c, "ids must be a nonempty list of photo ids"))?;
        ids.iter().map(|v| v.as_u64().map(PhotoId).ok_or_else(|| bad(c, "invalid photo id"))).collect::<Result<Vec<_>>>()?
    } else if view {
        s.visible_cloned()
    } else {
        s.selected()
    };
    ids.sort_unstable();
    ids.dedup();
    for id in &ids {
        if s.catalog.photo(*id).is_none() {
            return Err(bad(c, format!("no photo {}", id.0)));
        }
    }
    Ok(ids)
}

fn reindex(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = ids(s, p, "tags.reindex", true)?;
    let ops: Vec<_> = ids
        .iter()
        .filter_map(|id| {
            let photo = s.catalog.photo(*id)?;
            let mut meta = photo.meta.clone();
            let ai = meta.auto_tags.as_mut()?;
            ai.model.clear();
            Some(Op::SetMeta { id: *id, meta: Box::new(meta) })
        })
        .collect();
    if !ops.is_empty() {
        s.apply_system(Op::Batch { ops })?;
    }
    s.skip_auto_write = true;
    #[cfg(not(target_arch = "wasm32"))]
    {
        s.tagging.scan = Default::default();
    }
    Ok(json!({"queued":ids.len()}))
}

fn review(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "tags.review";
    let accept = match p.get("action").and_then(Value::as_str) {
        Some("accept") => true,
        Some("dismiss") => false,
        _ => return Err(bad(C, "action must be accept or dismiss")),
    };
    let names = p.get("names").map(|v| v.as_array().ok_or_else(|| bad(C, "names must be a list"))).transpose()?;
    if names.is_some_and(|ns| ns.iter().any(|n| n.as_str().is_none())) {
        return Err(bad(C, "keyword names must be strings"));
    }
    let ids = ids(s, p, C, false)?;
    let mut ops = Vec::new();
    let mut changed = 0;
    for id in ids {
        let Some(photo) = s.catalog.photo(id) else { continue };
        let candidates: Vec<_> = photo
            .suggested_tags()
            .filter(|t| names.is_none_or(|ns| ns.iter().any(|n| n.as_str().is_some_and(|n| lightcraft_catalog::keywords::same(n, &t.name)))))
            .map(|t| t.name.clone())
            .collect();
        if candidates.is_empty() {
            continue;
        }
        let mut meta = photo.meta.clone();
        let Some(ai) = meta.auto_tags.as_mut() else { continue };
        for name in &candidates {
            if accept {
                if !meta.keywords.iter().any(|k| lightcraft_catalog::keywords::same(k, name)) {
                    meta.keywords.push(name.clone());
                }
                for tag in &mut ai.tags {
                    if tag.name == *name {
                        tag.accepted = true;
                    }
                }
            } else if !ai.dismissed.iter().any(|n| lightcraft_catalog::keywords::same(n, name)) {
                ai.dismissed.push(name.clone());
            }
        }
        if !accept {
            ai.tags.retain(|t| !candidates.contains(&t.name));
        }
        changed += candidates.len();
        ops.push(Op::SetMeta { id, meta: Box::new(meta) });
    }
    if !ops.is_empty() {
        s.commit(if accept { "Accept automatic keywords" } else { "Dismiss automatic keywords" }, Op::Batch { ops })?;
    }
    s.skip_auto_write = !accept;
    Ok(json!({"changed":changed}))
}

#[cfg(target_arch = "wasm32")]
fn pump(_: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"active":false}))
}

#[cfg(not(target_arch = "wasm32"))]
fn ingest(s: &mut Session, done: crate::autotag::Done) -> Result<()> {
    s.tagging.scan.busy = false;
    if done.epoch != s.library_generation || !(s.tagging.settings.enabled || s.tagging.scan.manual) {
        return Ok(());
    }
    let Some(photo) = s.catalog.photo(done.id) else { return Ok(()) };
    if photo.content_key() != done.content {
        return Ok(());
    }
    let scores = match done.scores {
        Ok(scores) => scores,
        Err(error) => {
            if done.model_error {
                s.tagging.scan.error = Some(error.clone());
            }
            s.tagging.scan.failed.insert(done.id, (done.content, error));
            return Ok(());
        }
    };
    let mut meta = photo.meta.clone();
    let mut ai = meta.auto_tags.take().unwrap_or_default();
    ai.model = s.tagging.scan.model.clone();
    ai.content = done.content;
    ai.tags.retain(|t| t.accepted);
    let objects = s.tagging.settings.detector.as_deref().and_then(detector);
    let clip_key = model(&s.tagging.settings.model).map(|m| m.key()).unwrap_or_default();
    let object_key = objects.map(|m| m.key()).unwrap_or_default();
    let tags = lightcraft_autotag::select_context(&scores, s.tagging.settings.minimum, objects.is_some())
        .into_iter()
        .map(|(name, score)| (name, score, clip_key.clone()))
        .chain(done.objects.into_iter().map(|(name, score)| (name, score, object_key.clone())));
    for (name, score, model) in tags {
        if ai.dismissed.iter().any(|n| lightcraft_catalog::keywords::same(n, &name))
            || ai.tags.iter().any(|t| lightcraft_catalog::keywords::same(&t.name, &name))
        {
            continue;
        }
        ai.tags.push(AutoTag { name, score, model, accepted: false });
    }
    meta.auto_tags = Some(ai);
    s.apply_system(Op::SetMeta { id: done.id, meta: Box::new(meta) })?;
    s.skip_auto_write = true;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn pump(s: &mut Session, p: &Value) -> Result<Value> {
    use crate::autotag::{Work, Worker};
    finish_downloads(s);
    if s.tagging.scan.task.as_ref().is_some_and(|t| t.is_cancelled()) {
        return configure(s, &json!({"enabled":false}));
    }
    let view = view_key(s);
    if s.tagging.scan.view != view {
        s.tagging.scan.manual = false;
        s.tagging.scan.view = view;
        s.tagging.scan.queue.clear();
        s.tagging.scan.targets = s.visible_cloned().into_iter().collect();
        s.tagging.scan.stamp = None;
        s.tagging.scan.task = None;
        s.tagging.scan.peak = 0;
    }
    let result = s.tagging.scan.worker.as_ref().and_then(|w| w.receive.try_recv().ok());
    if let Some(done) = result {
        if s.tagging.scan.targets.contains(&done.id) && s.visible().contains(&done.id) {
            ingest(s, done)?;
        } else {
            s.tagging.scan.busy = false;
        }
    }
    if !(s.tagging.settings.enabled || s.tagging.scan.manual) {
        return Ok(json!({"active":false}));
    }
    let Some(m) = model(&s.tagging.settings.model) else {
        return Err(bad("tags.pump", "unknown model"));
    };
    let key = model_key(s);
    if s.tagging.scan.model != key {
        let manual = s.tagging.scan.manual;
        let worker = s.tagging.scan.worker.take();
        let busy = s.tagging.scan.busy;
        let view = s.tagging.scan.view.clone();
        let targets = std::mem::take(&mut s.tagging.scan.targets);
        s.tagging.scan = crate::autotag::Scan { manual, worker, busy, view, targets, model: key.clone(), ..Default::default() };
    }
    if s.tagging.scan.error.is_some() {
        s.tagging.scan.task = None;
        return Ok(json!({"active":true,"error":s.tagging.scan.error}));
    }
    if (s.tagging.scan.stamp.is_none() || (!s.tagging.scan.busy && s.tagging.scan.queue.is_empty()))
        && s.tagging.scan.stamp != Some(s.catalog.revision)
    {
        let visible = s.visible_cloned();
        s.tagging.scan.targets = visible.iter().copied().collect();
        s.tagging.scan.queue = visible
            .into_iter()
            .filter_map(|id| s.catalog.photo(id))
            .filter(|p| !p.deleted && p.kind != lightcraft_catalog::MediaKind::Video)
            .filter(|p| {
                let content = p.content_key();
                !p.meta.auto_tags.as_ref().is_some_and(|ai| ai.model == key && ai.content == content)
                    && !s.tagging.scan.failed.get(&p.id).is_some_and(|(c, _)| c == &content)
            })
            .map(|p| p.id)
            .collect();
        s.tagging.scan.stamp = Some(s.catalog.revision);
    }
    let pending = s.tagging.scan.queue.len() + usize::from(s.tagging.scan.busy);
    if pending > 0 {
        s.tagging.scan.peak = s.tagging.scan.peak.max(pending);
        let task = s.tagging.scan.task.get_or_insert_with(|| s.activity.start("tags", "Generating automatic keywords", crate::activity::Cancel::Yes));
        task.progress(s.tagging.scan.peak.saturating_sub(pending) as u64, s.tagging.scan.peak as u64);
    } else {
        s.tagging.scan.task = None;
        s.tagging.scan.peak = 0;
        s.tagging.scan.manual = false;
        if !s.tagging.settings.enabled {
            s.tagging.scan.worker = None;
        }
    }
    if p.get("pace").and_then(Value::as_str) != Some("pause") && !s.tagging.scan.busy {
        while let Some(id) = s.tagging.scan.queue.pop_front() {
            if !s.visible().contains(&id) {
                continue;
            }
            let Some(photo) = s.catalog.photo(id) else {
                continue;
            };
            if photo.deleted || !s.tagging.scan.targets.contains(&id) {
                continue;
            }
            let content = photo.content_key();
            if photo.meta.auto_tags.as_ref().is_some_and(|ai| ai.model == key && ai.content == content) {
                continue;
            }
            let Some(job) = s.preview_job(
                id,
                if s.tagging.settings.detector.is_some() { 640 } else { 512 },
                if s.tagging.settings.detector.is_some() { 640 } else { 512 },
                false,
                &lightcraft_develop::DevelopSettings::default(),
            ) else {
                continue;
            };
            if s.tagging.scan.worker.is_none() {
                match Worker::start(dir(s, "tags.pump")?, m, s.tagging.settings.detector.as_deref().and_then(detector)) {
                    Ok(worker) => s.tagging.scan.worker = Some(worker),
                    Err(e) => {
                        s.tagging.scan.error = Some(e);
                        break;
                    }
                }
            }
            let work = Work { id, epoch: s.library_generation, content, job };
            if s.tagging.scan.worker.as_ref().is_some_and(|w| w.send.try_send(work).is_ok()) {
                s.tagging.scan.busy = true;
            }
            break;
        }
    }
    Ok(json!({"active":true,"busy":s.tagging.scan.busy,"pending":pending,"failed":s.tagging.scan.failed.len(),"error":s.tagging.scan.error}))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::autotag::Done;
    use lightcraft_catalog::{Filter, Photo, Sort, Source};

    fn setup() -> Session {
        let mut s = Session::new();
        for id in 1..=2 {
            let mut p = Photo::new(PhotoId(id), Source::Demo { scene: 0 }, "plain.png", "PNG", 224, 224, "2026-10-19");
            p.meta.keywords.push("manual".into());
            s.catalog.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
        }
        s.selection.ids = vec![PhotoId(1), PhotoId(2)];
        s.selection.active = Some(PhotoId(1));
        s.tagging.settings.enabled = true;
        s.tagging.settings.model = "tinyclip-8m".into();
        s.tagging.settings.minimum = 0.34;
        s.tagging.scan.model = format!("{}:{}", MODELS[0].key(), s.tagging.settings.minimum);
        s
    }
    fn done(s: &Session, id: u64, name: &str) -> Done {
        let mut scores = vec![0.1; lightcraft_autotag::vocabulary().count()];
        let index = lightcraft_autotag::vocabulary().position(|(label, _)| label == name).unwrap();
        scores[index] = 0.35;
        Done {
            id: PhotoId(id),
            epoch: s.library_generation,
            content: s.catalog.photo(PhotoId(id)).unwrap().content_key(),
            scores: Ok(scores),
            objects: Vec::new(),
            model_error: false,
        }
    }
    fn names(s: &Session, id: u64) -> Vec<String> {
        s.catalog.photo(PhotoId(id)).unwrap().suggested_tags().map(|t| t.name.clone()).collect()
    }

    #[test]
    fn hybrid_tags_keep_detector_authority_context_provenance_and_review() {
        let mut s = setup();
        assert!(s.execute("tags.configure", &json!({"model":"yolox-s"})).is_err());
        assert!(s.execute("tags.configure", &json!({"detector":"tinyclip-8m"})).is_err());
        assert!(s.execute("tags.configure", &json!({"detector":"../../bad"})).is_err());
        s.tagging.settings.enabled = false;
        s.tagging.dir = None;
        assert!(s.execute("tags.configure", &json!({"detector":"yolox-s","enabled":true})).is_err());
        s.execute("tags.configure", &json!({"detector":"yolox-s"})).unwrap();
        assert_eq!(coverage(&mut s)["missing"], 2);
        assert!(s.execute("tags.scan", &json!({})).is_err());
        s.tagging.settings.enabled = true;
        s.tagging.scan.model = model_key(&s);
        let mut d = done(&s, 1, "dog");
        let scores = d.scores.as_mut().unwrap();
        scores[lightcraft_autotag::vocabulary().position(|(n, _)| n == "indoors").unwrap()] = 0.36;
        d.objects = vec![("cat".into(), 0.9), ("chair".into(), 0.8)];
        ingest(&mut s, d).unwrap();
        assert_eq!(names(&s, 1), ["indoors", "cat", "chair"]);
        let metadata = s.execute("photo.allMetadata", &json!({"id":1})).unwrap();
        let rows = metadata["catalog"].as_array().unwrap();
        assert!(rows.iter().any(|r| r["name"] == "AI tags" && r["value"].as_str().unwrap().contains("chair")));
        assert!(rows.iter().any(|r| r["name"] == "Keywords" && r["value"] == "manual"));
        assert!(rows.iter().any(|r| r["name"] == "AI tag models" && r["value"] == "TinyCLIP Compact; YOLOX Small"));
        assert!(!s.catalog.keyword_tree().iter().any(|n| n.name == "chair"));
        let p = s.catalog.photo(PhotoId(1)).unwrap();
        let tags = &p.meta.auto_tags.as_ref().unwrap().tags;
        assert!(tags.iter().filter(|t| t.name != "indoors").all(|t| t.model.starts_with("yolox-s@")));
        assert!(tags.iter().find(|t| t.name == "indoors").unwrap().model.starts_with("tinyclip-8m@"));
        assert_eq!(s.catalog.query(&Filter { text: "tag:cat tag:chair".into(), ..Default::default() }, &Sort::default()), [PhotoId(1)]);
        s.execute("tags.review", &json!({"ids":[1],"names":["cat"],"action":"accept"})).unwrap();
        s.execute("tags.review", &json!({"ids":[1],"names":["chair"],"action":"dismiss"})).unwrap();
        let key = model_key(&s);
        s.tagging.settings.detector = Some("yolox-nano".into());
        assert_ne!(key, model_key(&s));
        assert_eq!(coverage(&mut s)["tagged"], 0);
        s.tagging.scan.model = model_key(&s);
        let mut d = done(&s, 1, "dog");
        d.objects = vec![("chair".into(), 0.9)];
        ingest(&mut s, d).unwrap();
        assert!(s.catalog.photo(PhotoId(1)).unwrap().meta.keywords.contains(&"cat".into()));
        assert!(!names(&s, 1).contains(&"chair".into()));
        assert_eq!(coverage(&mut s)["tagged"], 1);
        let settings: crate::autotag::Settings = serde_json::from_value(json!({"enabled":false,"model":"tinyclip-8m","minimum":0.34})).unwrap();
        assert!(settings.detector.is_none(), "older preferences retain CLIP-only behavior");
        s.execute("tags.configure", &json!({"detector":null,"enabled":false})).unwrap();
        assert!(s.tagging.settings.detector.is_none());
    }

    #[test]
    fn queue_tracks_current_view_including_local_photos_and_subfolders() {
        let mut s = Session::new();
        for (id, path, local) in
            [(1, "/photos/a.jpg", true), (2, "/photos/child/b.jpg", true), (3, "/other/c.jpg", true), (4, "/library/d.jpg", false)]
        {
            let mut p = Photo::new(PhotoId(id), Source::File { path: path.into() }, path, "JPEG", 224, 224, "2026-10-19");
            p.local = local;
            s.catalog.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
        }
        s.tagging.settings.enabled = true;
        s.source = crate::LibrarySource::Folder;
        s.browse = Some(crate::Browse { path: "/photos".into(), subfolders: false });
        assert_eq!(s.execute("tags.pump", &json!({"pace":"pause"})).unwrap()["pending"], 1);
        assert!(s.tagging.scan.queue.contains(&PhotoId(1)));
        s.browse.as_mut().unwrap().subfolders = true;
        assert_eq!(s.execute("tags.pump", &json!({"pace":"pause"})).unwrap()["pending"], 2);
        s.filter.text = "b.jpg".into();
        assert_eq!(s.execute("tags.pump", &json!({"pace":"pause"})).unwrap()["pending"], 1);
        assert_eq!(s.tagging.scan.queue.front(), Some(&PhotoId(2)));
        s.execute("tags.reindex", &json!({})).unwrap();
        assert_eq!(s.execute("tags.pump", &json!({"pace":"pause"})).unwrap()["pending"], 1);
        s.filter = Filter::default();
        s.source = crate::LibrarySource::All;
        assert_eq!(s.execute("tags.pump", &json!({"pace":"pause"})).unwrap()["pending"], 1);
        assert_eq!(s.tagging.scan.queue.front(), Some(&PhotoId(4)));
        assert!(s.tagging.scan.worker.is_none());
    }

    #[test]
    fn previous_index_is_queued_and_review_decisions_survive_upgrade() {
        let mut s = setup();
        for id in 1..=2 {
            let d = done(&s, id, "cat");
            ingest(&mut s, d).unwrap();
        }
        s.execute("tags.review", &json!({"ids":[1],"action":"accept"})).unwrap();
        let p = s.catalog.photo(PhotoId(1)).unwrap();
        let mut meta = p.meta.clone();
        let ai = meta.auto_tags.as_mut().unwrap();
        ai.model = format!("{}@{}:1:{}", MODELS[0].id, MODELS[0].sha256, s.tagging.settings.minimum);
        ai.dismissed.push("chair".into());
        s.commit("old index fixture", Op::SetMeta { id: PhotoId(1), meta: Box::new(meta) }).unwrap();
        assert_eq!(coverage(&mut s)["tagged"], 1);
        assert_eq!(s.execute("tags.pump", &json!({"pace":"pause"})).unwrap()["pending"], 1);
        assert_eq!(s.tagging.scan.queue.front(), Some(&PhotoId(1)));
        let d = done(&s, 1, "chair");
        ingest(&mut s, d).unwrap();
        let p = s.catalog.photo(PhotoId(1)).unwrap();
        assert!(p.meta.keywords.contains(&"cat".into()));
        assert!(p.meta.auto_tags.as_ref().unwrap().dismissed.contains(&"chair".into()));
        assert!(!p.suggested_tags().any(|t| t.name == "chair"));
        assert_eq!(coverage(&mut s)["tagged"], 2);
    }

    #[test]
    fn coverage_counts_completed_images_and_new_arrivals_without_image_work() {
        let mut s = setup();
        assert_eq!(coverage(&mut s), json!({"tagged":0,"total":2,"missing":2}));
        let first = done(&s, 1, "cat");
        ingest(&mut s, first).unwrap();
        assert_eq!(coverage(&mut s)["tagged"], 1);
        let mut second = done(&s, 2, "cat");
        second.scores = Ok(vec![0.1; lightcraft_autotag::vocabulary().count()]);
        ingest(&mut s, second).unwrap();
        assert_eq!(coverage(&mut s), json!({"tagged":2,"total":2,"missing":0}), "zero-label results are complete");
        let photo = Photo::new(PhotoId(3), Source::Demo { scene: 0 }, "new.png", "PNG", 224, 224, "2026-10-19");
        s.catalog.apply(Op::AddPhoto { photo: Box::new(photo) }).unwrap();
        assert_eq!(coverage(&mut s), json!({"tagged":2,"total":3,"missing":1}));
        s.filter.text = "plain.png".into();
        assert_eq!(coverage(&mut s), json!({"tagged":2,"total":2,"missing":0}), "coverage follows filters");
        s.tagging.settings.minimum = 0.4;
        assert_eq!(coverage(&mut s)["tagged"], 0, "a different threshold needs a new scan");
        assert!(s.tagging.scan.worker.is_none());
    }

    #[test]
    fn one_shot_scan_processes_only_missing_without_enabling_automatic_tagging() {
        let mut s = setup();
        let first = done(&s, 1, "cat");
        ingest(&mut s, first).unwrap();
        s.tagging.settings.enabled = false;
        s.tagging.dir = None;
        assert!(s.execute("tags.scan", &json!({"rescan":true})).is_err(), "no model installed");
        assert_eq!(coverage(&mut s)["tagged"], 1, "failed starts must not invalidate completed images");
        let dir = crate::tests_xmp::temp_dir("one-shot-tags");
        let model_dir = dir.join(MODELS[0].id);
        std::fs::create_dir_all(&model_dir).unwrap();
        // Only the paused queue is exercised; this size-only fixture must never be loaded.
        std::fs::File::create(model_dir.join("model.safetensors")).unwrap().set_len(MODELS[0].bytes).unwrap();
        s.tagging.dir = Some(dir);
        assert_eq!(s.execute("tags.scan", &json!({})).unwrap()["queued"], 1);
        assert!(!s.tagging.settings.enabled);
        assert_eq!(s.execute("tags.pump", &json!({"pace":"pause"})).unwrap()["pending"], 1);
        assert_eq!(s.tagging.scan.queue.pop_front(), Some(PhotoId(2)));
        let second = done(&s, 2, "church");
        ingest(&mut s, second).unwrap();
        s.execute("tags.pump", &json!({"pace":"pause"})).unwrap();
        assert!(!s.tagging.scan.manual);
        assert!(!s.tagging.settings.enabled);
        assert_eq!(coverage(&mut s)["tagged"], 2);
        assert_eq!(s.execute("tags.scan", &json!({"rescan":true})).unwrap()["queued"], 2);
        assert_eq!(s.execute("tags.pump", &json!({"pace":"pause"})).unwrap()["pending"], 2);
        assert!(s.catalog.photo(PhotoId(1)).unwrap().meta.keywords.contains(&"manual".into()));
        s.filter.text = "no-matches".into();
        s.execute("tags.pump", &json!({"pace":"pause"})).unwrap();
        assert!(!s.tagging.scan.manual, "navigation stops a one-shot run");
        assert!(!s.tagging.settings.enabled);
    }

    #[test]
    fn review_search_undo_regeneration_and_persistence() {
        let dir = crate::tests_xmp::temp_dir("auto-keywords");
        let mut s = setup();
        s.open_library(dir.join("library"), false).unwrap();
        let seed = setup();
        for p in seed.catalog.photos() {
            s.commit("import", Op::AddPhoto { photo: Box::new(p.as_ref().clone()) }).unwrap();
        }
        s.tagging.settings.enabled = true;
        s.tagging.scan.model = seed.tagging.scan.model;
        s.selection = seed.selection;
        s.xmp.auto_write = true;
        let undo = s.undo.len();
        ingest(&mut s, done(&setup(), 1, "church")).unwrap();
        // Wrong library epoch is ignored.
        assert!(names(&s, 1).is_empty());
        let d = done(&s, 1, "church");
        ingest(&mut s, d).unwrap();
        let d = done(&s, 2, "church");
        ingest(&mut s, d).unwrap();
        assert_eq!(s.undo.len(), undo, "background scan adds no undo steps");
        assert_eq!(s.catalog.query(&Filter { text: "church".into(), ..Default::default() }, &Sort::default()).len(), 2);
        assert_eq!(s.execute("tags.review", &json!({"action":"accept","ids":[1,1]})).unwrap()["changed"], 1);
        assert!(s.catalog.photo(PhotoId(1)).unwrap().meta.keywords.contains(&"church".into()));
        assert!(s.catalog.photo(PhotoId(1)).unwrap().meta.auto_tags.as_ref().unwrap().tags[0].accepted);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(names(&s, 1), vec!["church"]);
        s.execute("edit.redo", &json!({})).unwrap();
        s.execute("tags.review", &json!({"action":"dismiss","ids":[2]})).unwrap();
        assert!(names(&s, 2).is_empty());
        s.execute("tags.reindex", &json!({})).unwrap();
        let d = done(&s, 2, "church");
        ingest(&mut s, d).unwrap();
        assert!(names(&s, 2).is_empty(), "dismissals survive rescans");
        s.save_prefs().unwrap();
        s.persist().unwrap();
        drop(s);
        let mut reopened = Session::new();
        reopened.open_library(dir.join("library"), false).unwrap();
        assert!(reopened.tagging.settings.enabled);
        assert!(reopened.catalog.photo(PhotoId(1)).unwrap().meta.keywords.contains(&"manual".into()));
        assert!(reopened.catalog.photo(PhotoId(1)).unwrap().meta.auto_tags.as_ref().unwrap().tags[0].accepted);
        assert!(reopened.catalog.photo(PhotoId(2)).unwrap().meta.auto_tags.as_ref().unwrap().dismissed.contains(&"church".into()));
    }
    #[test]
    fn stale_or_failed_results_never_replace_manual_work() {
        let mut s = setup();
        let mut d = done(&s, 1, "cat");
        d.content = "old".into();
        ingest(&mut s, d).unwrap();
        assert!(names(&s, 1).is_empty());
        let mut d = done(&s, 1, "cat");
        d.epoch += 1;
        ingest(&mut s, d).unwrap();
        assert!(names(&s, 1).is_empty());
        let mut d = done(&s, 1, "cat");
        d.scores = Err("offline".into());
        ingest(&mut s, d).unwrap();
        assert_eq!(s.tagging.scan.failed.len(), 1);
        assert!(names(&s, 1).is_empty());
        s.execute("tags.reindex", &json!({"ids":[1]})).unwrap();
        assert!(s.tagging.scan.failed.is_empty());
        let d = done(&s, 1, "cat");
        s.execute("photo.setMeta", &json!({"ids":[1],"title":"edited while working"})).unwrap();
        ingest(&mut s, d).unwrap();
        let p = s.catalog.photo(PhotoId(1)).unwrap();
        assert_eq!(p.meta.title, "edited while working");
        assert_eq!(p.meta.keywords, vec!["manual"]);
    }
    #[test]
    fn validation_pause_and_zero_tag_completion() {
        let mut s = setup();
        for (cmd, p) in [
            ("tags.scan", json!({"rescan":"yes"})),
            ("tags.configure", json!({"model":"../../evil"})),
            ("tags.configure", json!({"minimum":8})),
            ("tags.configure", json!({"enabled":"yes"})),
            ("tags.review", json!({"action":"drop"})),
            ("tags.review", json!({"action":"accept","ids":[999]})),
            ("tags.review", json!({"action":"accept","names":[7]})),
        ] {
            assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
        }
        s.execute("tags.pump", &json!({"pace":"pause"})).unwrap();
        assert!(!s.tagging.scan.busy);
        assert!(s.tagging.scan.worker.is_none());
        let mut d = done(&s, 1, "cat");
        d.scores = Ok(vec![0.1; lightcraft_autotag::vocabulary().count()]);
        ingest(&mut s, d).unwrap();
        assert!(names(&s, 1).is_empty());
        assert!(!s.catalog.photo(PhotoId(1)).unwrap().meta.auto_tags.as_ref().unwrap().model.is_empty());
        let task = s.tagging.scan.task.as_ref().unwrap().cancel_flag().unwrap();
        task.store(true, std::sync::atomic::Ordering::Relaxed);
        s.execute("tags.pump", &json!({})).unwrap();
        assert!(!s.tagging.settings.enabled);
    }

    #[test]
    fn large_library_search_and_paused_queue_do_no_image_work() {
        let mut s = Session::new();
        for id in 1..=20_000 {
            let mut p = Photo::new(PhotoId(id), Source::File { path: format!("/offline/{id}.jpg") }, "plain.jpg", "JPEG", 4000, 3000, "2026-10-19");
            p.meta.auto_tags = Some(lightcraft_catalog::autotags::AutoTags {
                model: "old-model".into(),
                content: p.content_key(),
                tags: vec![AutoTag { name: if id % 2 == 0 { "cat" } else { "dog" }.into(), score: 0.35, model: "old-model".into(), accepted: false }],
                dismissed: vec![],
            });
            s.catalog.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
        }
        let start = std::time::Instant::now();
        let found = s.catalog.query(&Filter { text: "CAT type:image".into(), ..Default::default() }, &Sort::default());
        assert_eq!(found.len(), 10_000);
        s.tagging.settings.enabled = true;
        let r = s.execute("tags.pump", &json!({"pace":"pause"})).unwrap();
        assert_eq!(r["pending"], 20_000);
        assert_eq!(coverage(&mut s)["total"], 20_000);
        assert_eq!(coverage(&mut s)["missing"], 20_000);
        assert!(!s.tagging.scan.busy);
        assert!(s.tagging.scan.worker.is_none());
        eprintln!("20,000 offline records: query + pending queue {:?}", start.elapsed());
    }
    #[test]
    fn real_background_index_preserves_original_and_resumes() {
        let Some(models) = std::env::var_os("LIGHTCRAFT_TAG_MODELS") else {
            lightcraft_testkit::skip!(Model, "set LIGHTCRAFT_TAG_MODELS for background indexing");
            return;
        };
        let dir = crate::tests_xmp::temp_dir("auto-keywords-background");
        let file = dir.join("untagged.png");
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, 224, 224);
            enc.set_color(png::ColorType::Rgb);
            enc.write_header().unwrap().write_image_data(&vec![128; 224 * 224 * 3]).unwrap();
        }
        std::fs::write(&file, &png).unwrap();
        let mut s = Session::new().with_fs();
        s.tagging.dir = Some(models.into());
        s.open_library(dir.join("library"), false).unwrap();
        s.execute("library.import", &json!({"paths":[file.to_string_lossy()],"local":true})).unwrap();
        s.source = crate::LibrarySource::Folder;
        s.browse = Some(crate::Browse { path: dir.to_string_lossy().into(), subfolders: false });
        s.execute("tags.configure", &json!({"enabled":true,"model":"tinyclip-8m"})).unwrap();
        s.execute("tags.pump", &json!({})).unwrap();
        assert!(s.tagging.scan.busy);
        // A result that finishes after navigation must not tag a photo outside the new view.
        s.filter.text = "does-not-match".into();
        let wait = std::time::Instant::now();
        loop {
            s.execute("tags.pump", &json!({"pace":"pause"})).unwrap();
            if !s.tagging.scan.busy {
                break;
            }
            assert!(wait.elapsed() < std::time::Duration::from_secs(60));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(s.catalog.photos().all(|p| p.meta.auto_tags.is_none()));
        s.filter = Filter::default();
        let started = std::time::Instant::now();
        loop {
            s.execute("tags.pump", &json!({})).unwrap();
            if s.catalog.photos().all(|p| p.meta.auto_tags.is_some()) {
                break;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(60), "background index timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(std::fs::read(&file).unwrap(), png);
        assert!(!file.with_extension("xmp").exists());
        s.persist().unwrap();
        drop(s);
        let mut reopened = Session::new();
        reopened.tagging.dir = std::env::var_os("LIGHTCRAFT_TAG_MODELS").map(Into::into);
        reopened.open_library(dir.join("library"), false).unwrap();
        let p = reopened.execute("tags.pump", &json!({"pace":"pause"})).unwrap();
        assert_eq!(p["pending"], 0);
        assert!(!reopened.tagging.scan.busy);
    }

    #[test]
    fn real_hybrid_background_search_and_reopen() {
        let Some(models) = std::env::var_os("LIGHTCRAFT_TAG_MODELS") else {
            lightcraft_testkit::skip!(Model, "set LIGHTCRAFT_TAG_MODELS for hybrid indexing");
            return;
        };
        let root = crate::tests_xmp::temp_dir("hybrid-index");
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../autotag/tests/fixtures");
        let mut paths = Vec::new();
        let mut originals = Vec::new();
        for (i, name) in ["cat-chair.jpg", "cat-chair-room.jpg", "cat-only.jpg", "camera-person-outdoors.jpg"].iter().enumerate() {
            let path = root.join(format!("{i}.jpg"));
            let bytes = std::fs::read(fixtures.join(name)).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            paths.push(path);
            originals.push(bytes);
        }
        let library = root.join("library");
        let mut s = Session::new().with_fs();
        s.tagging.dir = Some(models.clone().into());
        s.open_library(&library, false).unwrap();
        let imported = s.execute("library.import", &json!({"paths":paths})).unwrap();
        let ids: Vec<PhotoId> = imported["imported"].as_array().unwrap().iter().map(|v| PhotoId(v.as_u64().unwrap())).collect();
        s.execute("tags.configure", &json!({"model":"tinyclip-39m","detector":"yolox-s","enabled":false})).unwrap();
        s.execute("tags.scan", &json!({})).unwrap();
        let started = std::time::Instant::now();
        loop {
            let result = s.execute("tags.pump", &json!({})).unwrap();
            assert!(result["error"].is_null(), "{result}");
            if coverage(&mut s)["missing"] == 0 {
                break;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(90), "{result}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for text in ["cat chair", "tag:cat tag:chair", "CAT CHAIR"] {
            let mut found = s.catalog.query(&Filter { text: text.into(), ..Default::default() }, &Sort::default());
            found.sort_unstable();
            assert_eq!(found, ids[..2], "{text}");
        }
        assert!(names(&s, ids[3].0).contains(&"outdoors".into()), "{:?}", names(&s, ids[3].0));
        for id in &ids[..3] {
            assert!(s.catalog.photo(*id).unwrap().suggested_tags().any(|t| t.name == "cat" && t.model.starts_with("yolox-s@")));
        }
        s.persist().unwrap();
        drop(s);
        let mut reopened = Session::new();
        reopened.tagging.dir = Some(models.into());
        reopened.open_library(&library, false).unwrap();
        assert_eq!(reopened.tagging.settings.detector.as_deref(), Some("yolox-s"));
        assert_eq!(coverage(&mut reopened)["tagged"], 4);
        let mut found = reopened.catalog.query(&Filter { text: "tag:cat tag:chair".into(), ..Default::default() }, &Sort::default());
        found.sort_unstable();
        assert_eq!(found, ids[..2]);
        reopened.execute("tags.configure", &json!({"enabled":true})).unwrap();
        assert_eq!(reopened.execute("tags.pump", &json!({"pace":"pause"})).unwrap()["pending"], 0);
        assert!(reopened.tagging.scan.worker.is_none(), "completed images must not load models");
        for (path, bytes) in paths.iter().zip(originals) {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
            assert!(!path.with_extension("xmp").exists());
        }
    }

    #[test]
    #[ignore = "downloads 3.7 MB from the official YOLOX GitHub release"]
    fn real_detector_download_is_verified_and_installed() {
        let dir = crate::tests_xmp::temp_dir("autotag-detector-download");
        let mut s = Session::new();
        s.tagging.dir = Some(dir.clone());
        s.execute("tags.download", &json!({"model":"yolox-nano"})).unwrap();
        let started = std::time::Instant::now();
        loop {
            let value = s.execute("tags.models", &json!({})).unwrap();
            let row = &value["detectors"][0];
            assert_ne!(row["download"]["state"], "failed", "{row}");
            if row["installed"] == true {
                break;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(90), "{value}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        lightcraft_autotag::Detector::load(&dir, &DETECTORS[0]).unwrap();
        s.execute("tags.configure", &json!({"enabled":false,"detector":"yolox-nano"})).unwrap();
        assert!(installed(&s, &DETECTORS[0]));
    }

    #[test]
    #[ignore = "downloads 94 MB from huggingface.co"]
    fn real_model_download_is_verified_and_installed() {
        let dir = crate::tests_xmp::temp_dir("autotag-download");
        let mut s = Session::new();
        s.tagging.dir = Some(dir);
        s.tagging.settings.model = "tinyclip-8m".into();
        s.tagging.scan.error = Some("model needs downloading again".into());
        s.execute("tags.download", &json!({"model":"tinyclip-8m"})).unwrap();
        let start = std::time::Instant::now();
        loop {
            let rows = s.execute("tags.models", &json!({})).unwrap();
            let row = &rows["models"][0];
            assert_ne!(row["download"]["state"], "failed", "{row}");
            if row["installed"] == true {
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(180), "download timeout: {rows}");
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        lightcraft_autotag::Tagger::load(s.tagging.dir.as_ref().unwrap(), &MODELS[0]).unwrap();
        assert!(s.tagging.scan.error.is_none(), "a verified replacement clears a failed model");
        s.execute("tags.configure", &json!({"model":"tinyclip-8m","enabled":true})).unwrap();
    }
}
