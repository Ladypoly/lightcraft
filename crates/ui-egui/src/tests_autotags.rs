use crate::{LightcraftApp, Services, headless::Headless};
use lightcraft_catalog::{
    Op,
    autotags::{AutoTag, AutoTags},
};
use serde_json::json;
use std::time::Duration;

#[test]
fn automatic_keyword_panel_reviews_through_shared_commands() {
    let mut session = lightcraft_engine::Session::with_demo();
    let id = session.visible_cloned()[0];
    let photo = session.catalog.photo(id).unwrap();
    let mut meta = photo.meta.clone();
    meta.auto_tags = Some(AutoTags {
        model: "test-model".into(),
        content: photo.content_key(),
        tags: vec![AutoTag { name: "church".into(), score: 0.35, model: "test-model".into(), accepted: false }],
        dismissed: vec![],
    });
    session.commit("fixture", Op::SetMeta { id, meta: Box::new(meta) }).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services { png: None, ..Default::default() }), [1400.0, 1100.0], 1.0);
    let t = Duration::from_secs(30);
    for (method, p) in
        [("ui.set", json!({"view":"photoGrid","right":"keywords"})), ("engine.execute", json!({"command":"library.select","params":{"ids":[id.0]}}))]
    {
        let r = h.request(method, p, t);
        assert_eq!(r["ok"], true, "{r}");
    }
    h.settle(Duration::from_secs(90));
    if !h.app.widgets.iter().any(|(w, _)| w == "tags:coverage") {
        let r = h.request("ui.clickWidget", json!({"id":"section:automatic_keywords"}), t);
        assert_eq!(r["ok"], true, "{r}");
        h.step();
        h.settle(Duration::from_secs(30));
    }
    assert!(h.app.widgets.iter().any(|(w, _)| w == "tags:coverage"));
    assert!(h.app.widgets.iter().any(|(w, _)| w == "button:tags:acceptSelection"));
    assert!(h.app.widgets.iter().any(|(w, _)| w == "tags:filter:church"), "{:?}", h.app.widgets);
    assert!(h.painted_text().iter().any(|s| s.contains("AI · church")));
    assert!(!h.app.widgets.iter().any(|(w, _)| w == "keywordRow:church"));
    assert!(h.app.session.catalog.keyword_tree().iter().all(|n| n.name != "church"));
    assert_eq!(h.app.session.catalog.query(&lightcraft_catalog::Filter { text: "church".into(), ..Default::default() }, &Default::default()), [id]);
    let r = h.request("ui.clickWidget", json!({"id":"button:tags:accept:church"}), t);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    assert!(h.app.session.catalog.photo(id).unwrap().meta.keywords.contains(&"church".into()));
    h.settle(Duration::from_secs(30));
    assert!(h.app.widgets.iter().any(|(w, _)| w == "tags:accepted"));
    assert!(h.app.session.catalog.keyword_tree().iter().any(|n| n.name == "church" && n.count == 1));
    assert!(!h.app.widgets.iter().any(|(w, _)| w == "button:tags:acceptSelection"));
    let r = h.request("engine.execute", json!({"command":"edit.undo","params":{}}), t);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    let r = h.request("ui.clickWidget", json!({"id":"tags:dismiss:church"}), t);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    assert_eq!(h.app.session.catalog.photo(id).unwrap().suggested_tags().count(), 0);
    h.settle(Duration::from_secs(30));
    assert!(!h.app.widgets.iter().any(|(w, _)| w == "button:tags:dismissSelection"));
}

#[test]
fn automatic_keyword_panel_has_only_coverage_and_one_direct_action() {
    let session = lightcraft_engine::Session::with_demo();
    let mut h = Headless::new(LightcraftApp::new(session, Services { png: None, ..Default::default() }), [1100.0, 700.0], 1.0);
    let t = Duration::from_secs(30);
    let r = h.request("ui.set", json!({"view":"photoGrid","right":"keywords","rightWidth":269.0}), t);
    assert_eq!(r["ok"], true, "{r}");
    h.settle(t);
    let r = h.request("ui.clickWidget", json!({"id":"section:automatic_keywords"}), t);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    h.settle(t);
    let rect = |id| h.app.widgets.iter().find(|(w, _)| w == id).map(|(_, r)| *r).unwrap();
    assert!(rect("button:tags:scan").bottom() - rect("tags:coverage").top() < 70.0);
    assert!(rect("button:tags:scan").width() < 223.0);
    for id in ["tags:enabled", "tags:model", "tags:settings", "tags:actions", "button:tags:download"] {
        assert!(!h.app.widgets.iter().any(|(w, _)| w == id), "{id}");
    }
    assert!(h.painted_text().iter().any(|s| s == "Tag missing"));
    assert!(!h.painted_text().iter().any(|s| s.contains("Runs privately")));
    h.app.run("app.settings", json!({"tab":"tagging"})).unwrap();
    h.step();
    h.settle(t);
    assert!(h.app.widgets.iter().any(|(w, _)| w == "tags:enabled"));
    assert!(h.app.widgets.iter().any(|(w, _)| w == "tags:model"));
    let previous = h.app.session.tagging.settings.model.clone();
    let other = if previous == "tinyclip-8m" { "tinyclip-39m" } else { "tinyclip-8m" };
    for id in ["tags:model".to_string(), format!("tags:model:{other}")] {
        let r = h.request("ui.clickWidget", json!({"id":id}), t);
        assert_eq!(r["ok"], true, "{r}");
        h.step();
        h.settle(t);
    }
    assert_eq!(h.app.session.tagging.settings.model, other);
}

#[test]
fn automatic_keyword_progress_uses_the_shared_activity_task() {
    let mut session = lightcraft_engine::Session::with_demo();
    session.tagging.settings.enabled = true;
    let task = session.activity.start("tags", "Generating automatic keywords", lightcraft_engine::activity::Cancel::Yes);
    task.progress(4, 10);
    let mut h = Headless::new(LightcraftApp::new(session, Services { png: None, ..Default::default() }), [1100.0, 700.0], 1.0);
    // Exercise rendering without launching image inference or requiring downloaded weights.
    h.view.ctx.data_mut(|d| d.insert_temp(egui::Id::new("automatic-keywords-next-pump"), f64::MAX));
    let t = Duration::from_secs(30);
    let r = h.request("ui.set", json!({"view":"photoGrid","right":"keywords","rightWidth":269.0}), t);
    assert_eq!(r["ok"], true, "{r}");
    h.settle(Duration::from_secs(90));
    std::thread::sleep(Duration::from_millis(600));
    h.step();
    let bar = h.app.widgets.iter().find(|(w, _)| w == "tags:progress").unwrap().1;
    assert_eq!(bar.height(), 5.0);
    assert!(!h.app.widgets.iter().any(|(w, _)| w == &format!("activity:row:{}", task.id())));
    assert!(h.app.widgets.iter().any(|(w, _)| w == "button:tags:cancel"));
    assert!(!h.app.widgets.iter().any(|(w, _)| w == "button:tags:scan"));
    assert!(h.painted_text().iter().any(|s| s.ends_with("images tagged")));
    task.progress(7, 10);
    h.step();
    assert!(h.app.widgets.iter().any(|(w, _)| w == "tags:progress"));
    for right in ["info", "edit"] {
        assert_eq!(h.request("ui.set", json!({"right":right}), t)["ok"], true);
        h.step();
        h.step();
        assert!(!h.app.widgets.iter().any(|(w, _)| w == "button:tags:cancel"));
        assert!(h.app.widgets.iter().any(|(w, _)| w == &format!("activity:cancel:{}", task.id())));
    }
    assert_eq!(h.request("ui.set", json!({"right":"keywords"}), t)["ok"], true);
    h.step();
    assert_eq!(h.request("ui.clickWidget", json!({"id":"section:automatic_keywords"}), t)["ok"], true);
    h.step();
    assert!(h.app.widgets.iter().any(|(w, _)| w == &format!("activity:cancel:{}", task.id())));
    assert_eq!(h.request("ui.clickWidget", json!({"id":"section:automatic_keywords"}), t)["ok"], true);
    h.step();
    // This task is a fixture, not Scan's worker; cancellation must not start real inference.
    h.app.session.tagging.settings.enabled = false;
    let r = h.request("ui.clickWidget", json!({"id":"button:tags:cancel"}), t);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    assert!(task.is_cancelled());
    drop(task);
    h.step();
    assert!(!h.app.widgets.iter().any(|(w, _)| w == "tags:progress"));
    assert!(h.painted_text().iter().any(|s| s.ends_with("images tagged")));
}

#[test]
fn tagging_settings_show_download_and_retry_states_without_network() {
    let mut session = lightcraft_engine::Session::with_demo();
    let mut value = session.execute("tags.models", &json!({})).unwrap();
    let selected = session.tagging.settings.model.clone();
    let generation = session.library_generation();
    let mut h = Headless::new(LightcraftApp::new(session, Services { png: None, ..Default::default() }), [1100.0, 700.0], 1.0);
    let t = Duration::from_secs(30);
    h.view.ctx.data_mut(|d| d.insert_temp(egui::Id::new("automatic-keywords-next-pump"), f64::MAX));
    h.app.run("app.settings", json!({"tab":"tagging"})).unwrap();
    for state in ["missing", "running", "failed"] {
        let row = value["models"].as_array_mut().unwrap().iter_mut().find(|r| r["id"] == selected).unwrap();
        row["installed"] = json!(false);
        row["download"] = match state {
            "running" => json!({"state":"running","bytes":50,"total":100}),
            "failed" => json!({"state":"failed","error":"Test download failure"}),
            _ => json!(null),
        };
        h.view.ctx.data_mut(|d| d.insert_temp(egui::Id::new("automatic-keyword-model-rows"), (generation, f64::MAX, value.clone())));
        h.step();
        h.settle(t);
        let has = |id| h.app.widgets.iter().any(|(w, _)| w == id);
        assert_eq!(has("button:tags:download"), state != "running");
        assert_eq!(has("tags:downloadProgress"), state == "running");
        assert_eq!(has("button:tags:cancelDownload"), state == "running");
        assert_eq!(h.painted_text().iter().any(|s| s == "Model download failed."), state == "failed");
    }
}

#[test]
fn automatic_keyword_coverage_switches_between_tag_missing_and_rescan() {
    let mut session = lightcraft_engine::Session::with_demo();
    let mut rows = session.execute("tags.models", &json!({})).unwrap();
    let generation = session.library_generation();
    let mut h = Headless::new(LightcraftApp::new(session, Services { png: None, ..Default::default() }), [1100.0, 700.0], 1.0);
    let t = Duration::from_secs(30);
    h.request("ui.set", json!({"view":"photoGrid","right":"keywords"}), t);
    h.settle(t);
    let r = h.request("ui.clickWidget", json!({"id":"section:automatic_keywords"}), t);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    for (tagged, total, action) in [(800, 850, "Tag missing"), (850, 850, "Rescan"), (850, 851, "Tag missing")] {
        rows["coverage"] = json!({"tagged":tagged,"total":total,"missing":total-tagged});
        h.view.ctx.data_mut(|d| d.insert_temp(egui::Id::new("automatic-keyword-model-rows"), (generation, f64::MAX, rows.clone())));
        h.step();
        h.settle(t);
        assert!(h.painted_text().iter().any(|s| s == action));
        assert!(h.painted_text().iter().any(|s| s == &format!("{tagged} / {total} images tagged")));
    }
}

#[test]
fn tagging_settings_select_detector_without_adding_sidebar_controls() {
    let mut session = lightcraft_engine::Session::with_demo();
    // Keep generation paused: model installation is tested by the engine.
    session.tagging.settings.enabled = false;
    let mut h = Headless::new(LightcraftApp::new(session, Services { png: None, ..Default::default() }), [1400.0, 1100.0], 1.0);
    let t = Duration::from_secs(30);
    h.view.ctx.data_mut(|d| d.insert_temp(egui::Id::new("automatic-keywords-next-pump"), f64::MAX));
    h.app.run("app.settings", json!({"tab":"tagging"})).unwrap();
    h.step();
    h.settle(t);
    for id in ["tags:detector", "tags:detector:yolox-s"] {
        assert_eq!(h.request("ui.clickWidget", json!({"id":id}), t)["ok"], true);
        h.step();
        h.settle(t);
    }
    assert_eq!(h.app.session.tagging.settings.detector.as_deref(), Some("yolox-s"));
    assert!(h.painted_text().iter().any(|s| s.contains("YOLOX Small")));
    assert!(h.app.widgets.iter().any(|(id, _)| id == "tags:detector"));
    for id in ["tags:detector", "tags:detector:none"] {
        assert_eq!(h.request("ui.clickWidget", json!({"id":id}), t)["ok"], true);
        h.step();
        h.settle(t);
    }
    assert!(h.app.session.tagging.settings.detector.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn failed_tagging_pump_waits_before_retrying() {
    let mut session = lightcraft_engine::Session::with_demo();
    session.tagging.settings.enabled = true;
    session.tagging.settings.model = "unknown-test-model".into();
    let mut app = LightcraftApp::new(session, Default::default());
    let ctx = egui::Context::default();
    crate::panels::autotags::pump(&mut app, &ctx);
    let next = ctx.data(|d| d.get_temp::<f64>(egui::Id::new("automatic-keywords-next-pump"))).unwrap();
    assert_eq!(next, ctx.input(|i| i.time) + 5.0);
    crate::panels::autotags::pump(&mut app, &ctx);
    assert_eq!(ctx.data(|d| d.get_temp::<f64>(egui::Id::new("automatic-keywords-next-pump"))), Some(next));
}
