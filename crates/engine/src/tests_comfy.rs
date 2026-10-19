//! Import, upgrade, source replacement and durable ComfyUI search through public commands.

use lightcraft_catalog::{Filter, Op, PhotoId, Sort};
use serde_json::json;

use crate::Session;

fn png(prompt: Option<&str>, workflow: Option<&str>) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut bytes, 2, 1);
        enc.set_color(png::ColorType::Rgb);
        if let Some(prompt) = prompt {
            enc.add_text_chunk("prompt".into(), prompt.into()).unwrap();
        }
        if let Some(workflow) = workflow {
            enc.add_text_chunk("workflow".into(), workflow.into()).unwrap();
        }
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(&[10, 20, 30, 40, 50, 60]).unwrap();
    }
    bytes
}

fn query(s: &Session, text: &str) -> Vec<PhotoId> {
    s.catalog.query(&Filter { text: text.into(), ..Default::default() }, &Sort::default())
}

#[test]
fn all_metadata_includes_raw_png_generation_fields_and_readable_summary() {
    let dir = crate::tests_xmp::temp_dir("comfy-viewer");
    let file = dir.join("generated.png");
    let bytes =
        png(Some(include_str!("../../meta/tests/fixtures/comfy-api.json")), Some(include_str!("../../meta/tests/fixtures/comfy-workflow.json")));
    std::fs::write(&file, &bytes).unwrap();
    let mut s = Session::new().with_fs();
    s.execute("library.import", &json!({"paths":[file.to_string_lossy()]})).unwrap();
    let id = s.catalog.photos().next().unwrap().id;
    let rows = s.execute("photo.allMetadata", &json!({"id":id.0})).unwrap();
    assert!(rows["exif"].as_array().unwrap().is_empty());
    assert_eq!(rows["pngText"].as_array().unwrap().len(), 2);
    assert!(rows["pngText"].as_array().unwrap().iter().any(|r| r["name"] == "workflow"));
    assert!(rows["generation"].as_array().unwrap().iter().any(|r| r["name"] == "node" && r["value"].as_str().unwrap().contains("MyCustomNode")));
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
}

#[test]
fn comfy_import_reindex_reopen_and_replace_sources() {
    let dir = crate::tests_xmp::temp_dir("comfy-lifecycle");
    let path = dir.join("generated.png");
    let library = dir.join("library");
    let original = png(Some(include_str!("../../meta/tests/fixtures/comfy-api.json")), None);
    std::fs::write(&path, &original).unwrap();
    let mut s = Session::new().with_fs();
    s.open_library(&library, false).unwrap();
    let imported = s.execute("library.import", &json!({"paths": [path.to_string_lossy()]})).unwrap();
    assert_eq!(imported["imported"].as_array().unwrap().len(), 1, "{imported}");
    let id = s.catalog.photos().next().unwrap().id;
    assert_eq!(query(&s, "prompt:\"neon city\" model:flux lora:detail seed:18446744073709551614"), vec![id]);
    s.execute("library.select", &json!({"ids": [id.0]})).unwrap();
    s.execute("photo.setMeta", &json!({"title": "Keep my title", "keywords": ["portfolio"]})).unwrap();
    s.execute("photo.rate", &json!({"rating": 4})).unwrap();
    s.execute("develop.set", &json!({"control": "light.exposure", "value": 0.5})).unwrap();

    // A persisted record from before generation metadata was extracted, with unchanged source bytes.
    let mut old = s.catalog.photo(id).unwrap().meta.clone();
    old.comfy = None;
    s.commit("Old metadata fixture", Op::SetMeta { id, meta: Box::new(old) }).unwrap();
    s.persist().unwrap();
    let hash = s.catalog.photo(id).unwrap().content_hash.clone();
    drop(s);
    let mut s = Session::new().with_fs();
    s.open_library(&library, false).unwrap();
    assert!(query(&s, "comfy:true").is_empty());
    s.execute("library.xmpPreferences", &json!({"autoWrite": true})).unwrap();
    let r = s.execute("photo.reload", &json!({"ids": [id.0]})).unwrap();
    assert_eq!(r["reloaded"], json!([id.0]));
    assert_eq!(r["failed"], json!([]));
    assert_eq!(s.catalog.photo(id).unwrap().content_hash, hash);
    let p = s.catalog.photo(id).unwrap();
    assert_eq!(p.meta.title, "Keep my title");
    assert_eq!(p.rating, 4);
    assert_eq!(p.develop.light.exposure, 0.5);
    assert_eq!(query(&s, "comfy:true portfolio"), vec![id]);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(!path.with_extension("xmp").exists(), "reindex never auto-writes sidecars");
    let r = s.execute("photo.reload", &json!({"ids": [id.0]})).unwrap();
    assert_eq!(r["reloaded"], json!([]), "unchanged metadata is not journalled twice");
    drop(s);
    let mut s = Session::new().with_fs();
    s.open_library(&library, false).unwrap();
    assert_eq!(query(&s, "comfy:true cfg:>5"), vec![id]);

    std::fs::remove_file(&path).unwrap();
    assert_eq!(query(&s, "sampler:euler"), vec![id], "offline search only reads catalog data");
    let r = s.execute("photo.reload", &json!({"ids": [id.0]})).unwrap();
    assert_eq!(r["failed"].as_array().unwrap().len(), 1);
    assert_eq!(query(&s, "comfy:true"), vec![id], "unreadable sources keep their previous metadata");

    let updated = png(None, Some(include_str!("../../meta/tests/fixtures/comfy-workflow.json")));
    std::fs::write(&path, &updated).unwrap();
    s.execute("photo.reload", &json!({"ids": [id.0]})).unwrap();
    assert_eq!(query(&s, "comfy:true steps:28 seed:123456"), vec![id]);
    assert!(query(&s, "seed:18446744073709551614").is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), updated);

    std::fs::write(&path, png(None, None)).unwrap();
    s.execute("photo.reload", &json!({"ids": [id.0]})).unwrap();
    assert!(query(&s, "comfy:true").is_empty());
    assert_eq!(query(&s, "comfy:false portfolio"), vec![id]);
    assert_eq!(s.catalog.photo(id).unwrap().meta.title, "Keep my title");
    assert!(!path.with_extension("xmp").exists());
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn comfy_reload_updates_virtual_copies_and_is_undoable() {
    let mut s = Session::new();
    let id = s.catalog.alloc_photo_id();
    let mut p =
        lightcraft_catalog::Photo::new(id, lightcraft_catalog::Source::File { path: "/generated.png".into() }, "old.png", "PNG", 2, 1, "2026-10-18");
    p.meta.title = "Original".into();
    s.catalog.apply(Op::AddPhoto { photo: Box::new(p.clone()) }).unwrap();
    let copy = s.catalog.alloc_photo_id();
    p.id = copy;
    p.copy_of = Some(id);
    p.meta.title = "Copy's title".into();
    s.catalog.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
    let info = crate::files::probe_bytes("generated.png", &png(Some(include_str!("../../meta/tests/fixtures/comfy-api.json")), None)).unwrap();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = calls.clone();
    s.media.file_probe = Some(std::sync::Arc::new(move |_| {
        counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(info.clone())
    }));
    s.execute("library.select", &json!({"ids": [copy.0, id.0]})).unwrap();
    let r = s.execute("photo.reload", &json!({"ids": [copy.0, id.0]})).unwrap();
    assert_eq!(r["reloaded"], json!([id.0, copy.0]));
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1, "a shared source is probed once");
    assert_eq!(query(&s, "comfy:true").len(), 2);
    assert_eq!(s.catalog.photo(copy).unwrap().meta.title, "Copy's title");
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(query(&s, "comfy:true").is_empty());
}

#[test]
fn comfy_uses_existing_library_filter_and_catalog_query_commands() {
    let mut s = Session::new();
    let info = crate::files::probe_bytes("generated.png", &png(Some(include_str!("../../meta/tests/fixtures/comfy-api.json")), None)).unwrap();
    let id = s.catalog.alloc_photo_id();
    let mut p = lightcraft_catalog::Photo::new(id, lightcraft_catalog::Source::Demo { scene: 0 }, "generated.png", "PNG", 2, 1, "2026-10-18");
    p.meta = info.meta;
    s.catalog.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
    let text = "comfy:true prompt:\"NEON CITY\" model:flux steps:>20 cfg:>5";
    s.execute("library.filter", &json!({"text": text})).unwrap();
    assert_eq!(s.visible_cloned(), vec![id]);
    let r = s.execute("catalog.query", &json!({})).unwrap();
    assert_eq!(r["total"], 1);
    let r = s.execute("catalog.query", &json!({"filter": {"text": "seed:18446744073709551614"}})).unwrap();
    assert_eq!(r["total"], 1);
    s.execute("library.filter", &json!({"text": "prompt:missing"})).unwrap();
    assert!(s.visible_cloned().is_empty());
}

#[test]
fn comfy_reindexes_a_large_existing_library_in_one_batch() {
    const COUNT: usize = 20_000;
    let mut s = Session::new();
    let info = crate::files::probe_bytes("generated.png", &png(Some(include_str!("../../meta/tests/fixtures/comfy-api.json")), None)).unwrap();
    let mut ids = Vec::new();
    for i in 0..COUNT {
        let id = s.catalog.alloc_photo_id();
        let p = lightcraft_catalog::Photo::new(
            id,
            lightcraft_catalog::Source::File { path: format!("/images/{i}.png") },
            "generated.png",
            "PNG",
            2,
            1,
            "2026-10-18",
        );
        s.catalog.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
        ids.push(id);
    }
    s.media.file_probe = Some(std::sync::Arc::new(move |_| Ok(info.clone())));
    s.selection = crate::Selection { active: ids.first().copied(), ids };
    let started = std::time::Instant::now();
    let r = s.execute("photo.reload", &json!({})).unwrap();
    assert_eq!(r["reloaded"].as_array().unwrap().len(), COUNT);
    assert_eq!(r["failed"], json!([]));
    assert_eq!(query(&s, "comfy:true model:flux prompt:\"neon city\"").len(), COUNT);
    eprintln!("ComfyUI catalog update and query over 20,000 existing records (probes supplied): {:?}", started.elapsed());
}
