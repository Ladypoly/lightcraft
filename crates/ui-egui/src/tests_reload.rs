use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use lightcraft_catalog::{Meta, Op, Photo, PhotoId, Source};
use lightcraft_engine::media::ProbeInfo;
use serde_json::json;

use crate::LightcraftApp;

#[derive(Default)]
struct Gate {
    entered: AtomicUsize,
    open: Mutex<bool>,
    ready: Condvar,
}

impl Gate {
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.ready.notify_all();
    }

    fn wait_started(&self) {
        let start = std::time::Instant::now();
        while self.entered.load(Ordering::Relaxed) == 0 {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn app() -> (LightcraftApp, Arc<Gate>) {
    let mut session = lightcraft_engine::Session::new().with_fs();
    for id in 1..=2 {
        let path = format!("/photos/{id}.png");
        let mut photo = Photo::new(PhotoId(id), Source::File { path }, &format!("{id}.png"), "PNG", 2, 1, "now");
        photo.content_hash = Some("old".into());
        session.catalog.apply(Op::AddPhoto { photo: Box::new(photo) }).unwrap();
    }
    session.execute("library.select", &json!({"ids":[1,2]})).unwrap();
    let gate = Arc::new(Gate::default());
    let wait = gate.clone();
    session.media.file_probe = Some(Arc::new(move |_| {
        wait.entered.fetch_add(1, Ordering::Relaxed);
        let (open, timeout) = wait.ready.wait_timeout_while(wait.open.lock().unwrap(), Duration::from_secs(10), |open| !*open).unwrap();
        assert!(*open && !timeout.timed_out(), "UI must return while disk I/O is blocked");
        Ok(ProbeInfo {
            width: 2,
            height: 1,
            format: "PNG".into(),
            content_hash: Some("new".into()),
            meta: Meta { camera: "refreshed".into(), ..Default::default() },
            ..Default::default()
        })
    }));
    (LightcraftApp::new(session, Default::default()), gate)
}

#[test]
fn batch_reload_keeps_ui_responsive_and_preserves_late_edits_in_one_undo() {
    let (mut app, gate) = app();
    assert_eq!(app.run("photo.reload", json!({})).unwrap()["started"], true);
    gate.wait_started();
    let task = app.session.activity.list().into_iter().find(|t| t.kind == "reload").unwrap();
    assert_eq!((task.done, task.total, task.cancellable), (0, 2, true));
    assert!(app.run("photo.reload", json!({})).is_err(), "no overlapping reloads");
    app.run("photo.setMeta", json!({"title":"Typed while probing"})).unwrap();
    let undo = app.session.undo.len();
    gate.release();
    assert!(crate::tasks::wait(&mut app, &egui::Context::default(), Duration::from_secs(10)));
    for id in [PhotoId(1), PhotoId(2)] {
        let p = app.session.catalog.photo(id).unwrap();
        assert_eq!(p.meta.title, "Typed while probing");
        assert_eq!(p.meta.camera, "refreshed");
        assert_eq!(p.content_hash.as_deref(), Some("new"));
    }
    assert_eq!(app.session.undo.len(), undo + 1);
    assert!(app.session.activity.list().is_empty());
    app.run("edit.undo", json!({})).unwrap();
    for p in app.session.catalog.photos() {
        assert!(p.meta.camera.is_empty());
        assert_eq!(p.meta.title, "Typed while probing");
        assert_eq!(p.content_hash.as_deref(), Some("old"));
    }
}

#[test]
fn cancelled_batch_reload_keeps_catalog_unchanged() {
    let (mut app, gate) = app();
    let before = app.session.catalog.to_snapshot();
    app.run("photo.reload", json!({})).unwrap();
    gate.wait_started();
    let task = app.session.activity.list()[0].id;
    app.run("activity.cancel", json!({"id":task})).unwrap();
    gate.release();
    assert!(crate::tasks::wait(&mut app, &egui::Context::default(), Duration::from_secs(10)));
    assert_eq!(app.session.catalog.to_snapshot(), before);
    assert!(app.session.undo.is_empty());
    assert!(app.session.activity.list().is_empty());
}

#[test]
fn batch_reload_drops_removed_replaced_and_other_library_results() {
    let (mut app, gate) = app();
    app.run("photo.reload", json!({})).unwrap();
    gate.wait_started();
    let mut changed = (**app.session.catalog.photo(PhotoId(1)).unwrap()).clone();
    changed.content_hash = Some("replacement".into());
    app.session.catalog.apply(Op::RemovePhoto { id: PhotoId(1) }).unwrap();
    app.session.catalog.apply(Op::AddPhoto { photo: Box::new(changed) }).unwrap();
    app.session.catalog.apply(Op::RemovePhoto { id: PhotoId(2) }).unwrap();
    gate.release();
    assert!(crate::tasks::wait(&mut app, &egui::Context::default(), Duration::from_secs(10)));
    let p = app.session.catalog.photo(PhotoId(1)).unwrap();
    assert!(p.meta.camera.is_empty());
    assert_eq!(p.content_hash.as_deref(), Some("replacement"));
    assert!(app.session.undo.is_empty());

    let (mut app, gate) = self::app();
    app.run("photo.reload", json!({})).unwrap();
    gate.wait_started();
    let dir = std::env::temp_dir().join(format!("lc-ui-reload-switch-{}", std::process::id()));
    app.session.open_library(&dir, false).unwrap();
    let before = app.session.catalog.to_snapshot();
    gate.release();
    assert!(crate::tasks::wait(&mut app, &egui::Context::default(), Duration::from_secs(10)));
    assert_eq!(app.session.catalog.to_snapshot(), before);
    assert!(app.session.undo.is_empty());
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn batch_reload_does_not_suppress_the_next_edits_automatic_xmp_write() {
    let (mut app, gate) = app();
    let dir = std::env::temp_dir().join(format!("lc-ui-reload-xmp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for id in [PhotoId(1), PhotoId(2)] {
        let mut photo = (**app.session.catalog.photo(id).unwrap()).clone();
        let path = dir.join(&photo.file_name);
        std::fs::write(&path, b"source bytes remain unchanged").unwrap();
        photo.source = Source::File { path: path.to_string_lossy().into_owned() };
        app.session.catalog.apply(Op::RemovePhoto { id }).unwrap();
        app.session.catalog.apply(Op::AddPhoto { photo: Box::new(photo) }).unwrap();
    }
    app.run("library.xmpPreferences", json!({"autoWrite":true})).unwrap();
    app.run("photo.reload", json!({"ids":[1,2]})).unwrap();
    gate.wait_started();
    gate.release();
    assert!(crate::tasks::wait(&mut app, &egui::Context::default(), Duration::from_secs(10)));
    assert!(!dir.join("1.xmp").exists());
    assert!(!dir.join("2.xmp").exists());
    app.run("photo.setMeta", json!({"ids":[1],"title":"Persist the next edit"})).unwrap();
    let packet = std::fs::read_to_string(dir.join("1.xmp")).unwrap();
    assert_eq!(lightcraft_meta::parse_xmp(&packet).unwrap().metadata.title.as_deref(), Some("Persist the next edit"));
    assert!(!dir.join("2.xmp").exists());
    assert_eq!(std::fs::read(dir.join("1.png")).unwrap(), b"source bytes remain unchanged");
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
