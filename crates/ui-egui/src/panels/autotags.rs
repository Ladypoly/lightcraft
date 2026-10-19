//! Automatic keywords live alongside the existing keyword editor.
use crate::{
    LightcraftApp,
    theme::Tokens,
    widgets::{register, text_button},
};
use serde_json::{Value, json};

type ModelRows = (u64, f64, Value);
const MODEL_ROWS: &str = "automatic-keyword-model-rows";

pub fn section(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    let key = egui::Id::new("automatic-keywords-initial-fold").with(app.session.library_generation());
    if !ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false) {
        let suggestions = app.session.active().and_then(|id| app.session.catalog.photo(id)).is_some_and(|p| p.suggested_tags().next().is_some());
        if !app.session.tagging.settings.enabled && !suggestions && !app.ui.section_folded("automatic_keywords") {
            app.ui.toggle_folded("automatic_keywords");
        }
        ui.ctx().data_mut(|d| d.insert_temp(key, true));
    }
    if super::right::fold_header(app, ui, "automatic_keywords", "Automatic Keywords") {
        super::right::padded(ui, |ui| show(app, ui));
    }
}

fn model_rows(app: &mut LightcraftApp, ctx: &egui::Context) -> Option<Value> {
    let key = egui::Id::new(MODEL_ROWS);
    let now = ctx.input(|i| i.time);
    let generation = app.session.library_generation();
    if let Some((library, until, value)) = ctx.data(|d| d.get_temp::<ModelRows>(key))
        && library == generation
        && now < until
    {
        return Some(value);
    }
    let value = app.session.execute("tags.models", &json!({})).ok()?;
    ctx.data_mut(|d| d.insert_temp(key, (generation, now + 0.5, value.clone())));
    Some(value)
}

pub fn pump(app: &mut LightcraftApp, ctx: &egui::Context) {
    let key = egui::Id::new("automatic-keywords-next-pump");
    let now = ctx.input(|i| i.time);
    if ctx.data(|d| d.get_temp::<f64>(key)).is_some_and(|next| now < next) {
        return;
    }
    let pace = super::faces::background_pace(app, ctx, now);
    let Ok(v) = app.session.execute("tags.pump", &json!({"pace":pace})) else {
        return;
    };
    let downloading = app.session.tagging.downloads.running();
    let active = v["active"] == true;
    let busy = v["busy"] == true || v["pending"].as_u64().unwrap_or(0) > 0;
    let delay = if busy || downloading { 0.1 } else { 5.0 };
    ctx.data_mut(|d| d.insert_temp(key, now + delay));
    if active || downloading {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay));
    }
}

fn run(app: &mut LightcraftApp, ui: &egui::Ui, command: &str, params: Value) {
    if let Err(e) = app.run(command, params) {
        app.toast_error(ui.ctx(), e);
    }
    ui.ctx().data_mut(|d| d.remove::<f64>(egui::Id::new("automatic-keywords-next-pump")));
    ui.ctx().data_mut(|d| d.remove::<ModelRows>(egui::Id::new(MODEL_ROWS)));
}

pub fn settings_tab(app: &mut LightcraftApp, ui: &mut egui::Ui, t: &Tokens) {
    egui::ScrollArea::vertical()
        .id_salt("tagging-settings-scroll")
        .max_height((ui.ctx().content_rect().height() - 250.0).max(180.0))
        .show(ui, |ui| settings_contents(app, ui, t));
}

fn settings_contents(app: &mut LightcraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some(value) = model_rows(app, ui.ctx()) else { return };
    super::settings::heading(ui, t, "Automatic Keywords");
    super::settings::hint(ui, t, "Tag photos locally from their visual content. No images are uploaded.");
    if value["available"] != true {
        ui.label(crate::i18n::tr("Automatic keyword generation requires the desktop app. Existing tags remain searchable."));
        return;
    }
    let current = app.session.tagging.settings.clone();
    let rows = value["models"].as_array().cloned().unwrap_or_default();
    let installed = ready(&value, &current);
    let mut enabled = current.enabled;
    let toggle = ui
        .add_enabled(installed, egui::Checkbox::new(&mut enabled, crate::i18n::tr("Auto-tag new images")))
        .on_hover_text(crate::i18n::tr(
            "Automatically process missing photos in the current view, including newly added images. Respects folders, subfolders and filters.",
        ))
        .on_disabled_hover_text(crate::i18n::tr("Download the selected model to enable auto-tagging."));
    register(ui.ctx(), "tags:enabled", toggle.rect);
    if toggle.changed() {
        run(app, ui, "tags.configure", json!({"enabled":enabled}));
    }
    ui.add_space(8.0);
    super::settings::heading(ui, t, "Scene model (CLIP)");
    let selected = rows.iter().find(|r| r["id"] == current.model).and_then(|r| r["name"].as_str()).unwrap_or("Choose a model");
    let combo = egui::ComboBox::from_id_salt("automatic-keyword-model")
        .selected_text(selected.trim_start_matches("TinyCLIP "))
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for row in &rows {
                let Some(id) = row["id"].as_str() else { continue };
                let name = row["name"].as_str().unwrap_or(id).trim_start_matches("TinyCLIP ");
                let description = row["description"].as_str().unwrap_or("");
                let label = format!("{name} · {:.0} MB", row["bytes"].as_u64().unwrap_or(0) as f64 / 1_000_000.0);
                let choice = ui.selectable_label(current.model == id, label).on_hover_text(description);
                register(ui.ctx(), format!("tags:model:{id}"), choice.rect);
                if choice.clicked() {
                    run(app, ui, "tags.configure", json!({"model":id,"enabled":current.enabled && row["installed"]==true}));
                }
            }
        });
    register(ui.ctx(), "tags:model", combo.response.rect);
    crate::widgets::screen_reader_name(ui.ctx(), combo.response.id, crate::i18n::tr("Tagging model"));
    combo.response.on_hover_text(crate::i18n::tr("Tagging model. Balanced offers better recognition; Compact uses less memory and CPU."));
    let selected = rows.iter().find(|r| r["id"] == app.session.tagging.settings.model);
    if let Some(row) = selected {
        ui.label(crate::i18n::tr(if row["id"] == "tinyclip-8m" {
            "Uses less memory and CPU, with noisier suggestions."
        } else {
            "Better recognition; uses more memory and CPU."
        }));
        let state = if row["installed"] == true { crate::i18n::tr("installed") } else { crate::i18n::tr("Not downloaded") };
        ui.label(format!("{:.0} MB · MIT · CPU · {state}", row["bytes"].as_u64().unwrap_or(0) as f64 / 1_000_000.0));
        if let Some(source) = row["source"].as_str() {
            ui.hyperlink_to(crate::i18n::tr("Model page and licence"), source);
        }
        ui.add_space(6.0);
        download_controls(app, ui, row, &value, "tags");
    }
    ui.add_space(12.0);
    super::settings::heading(ui, t, "Object detector (YOLOX)");
    let objects = value["detectors"].as_array().cloned().unwrap_or_default();
    let selected = current.detector.as_deref().and_then(|id| objects.iter().find(|r| r["id"] == id));
    let combo = egui::ComboBox::from_id_salt("automatic-keyword-detector")
        .selected_text(selected.and_then(|r| r["name"].as_str()).unwrap_or("Off — CLIP only"))
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            let off = ui.selectable_label(current.detector.is_none(), crate::i18n::tr("Off — CLIP only"));
            register(ui.ctx(), "tags:detector:none", off.rect);
            if off.clicked() {
                run(app, ui, "tags.configure", json!({"detector":null}));
            }
            for row in &objects {
                let Some(id) = row["id"].as_str() else { continue };
                let label = format!("{} · {:.0} MB", row["name"].as_str().unwrap_or(id), row["bytes"].as_u64().unwrap_or(0) as f64 / 1_000_000.0);
                let choice =
                    ui.selectable_label(current.detector.as_deref() == Some(id), label).on_hover_text(row["description"].as_str().unwrap_or(""));
                register(ui.ctx(), format!("tags:detector:{id}"), choice.rect);
                if choice.clicked() {
                    run(app, ui, "tags.configure", json!({"detector":id,"enabled":current.enabled && row["installed"]==true}));
                }
            }
        });
    register(ui.ctx(), "tags:detector", combo.response.rect);
    crate::widgets::screen_reader_name(ui.ctx(), combo.response.id, crate::i18n::tr("Object detector"));
    if let Some(row) = selected {
        ui.label(crate::i18n::tr(if row["id"] == "yolox-nano" {
            "Faster; may miss smaller or obscured objects."
        } else {
            "Better object detection; more CPU time."
        }));
        let state = if row["installed"] == true { "installed" } else { "Not downloaded" };
        ui.label(format!("{:.0} MB · Apache-2.0 · CPU · {}", row["bytes"].as_u64().unwrap_or(0) as f64 / 1_000_000.0, crate::i18n::tr(state)));
        if let Some(source) = row["source"].as_str() {
            ui.hyperlink_to(crate::i18n::tr("Model page and licence"), source);
        }
        download_controls(app, ui, row, &value, "tags:detector");
    }
    super::settings::hint(
        ui,
        t,
        "YOLOX finds 80 common object types; CLIP supplies scenes, activities and styles. Changing models makes images eligible for Tag missing again.",
    );
    ui.add_space(12.0);
    super::settings::heading(ui, t, "Tagging and review");
    ui.label(crate::i18n::tr("Use Tag missing in the Keywords panel to process unscanned images once. Rescan processes the current view again."));
    super::settings::hint(
        ui,
        t,
        "Tags are searchable immediately. Save Metadata to File preserves them separately in XMP. Accept adds ordinary keywords; dismiss hides them.",
    );
}

fn score_label(model: &str, score: f32) -> String {
    if model.starts_with("yolox-") {
        format!("Object confidence: {:.0}%", score * 100.0)
    } else {
        format!("Cosine similarity: {score:.3} (not a probability)")
    }
}

fn ready(value: &Value, settings: &lightcraft_engine::autotag::Settings) -> bool {
    let clip = value["models"].as_array().is_some_and(|rows| rows.iter().any(|r| r["id"] == settings.model && r["installed"] == true));
    clip && settings
        .detector
        .as_deref()
        .is_none_or(|id| value["detectors"].as_array().is_some_and(|rows| rows.iter().any(|r| r["id"] == id && r["installed"] == true)))
}

fn download_controls(app: &mut LightcraftApp, ui: &mut egui::Ui, row: &Value, value: &Value, prefix: &str) {
    let Some(id) = row["id"].as_str() else { return };
    if row["download"]["state"] == "running" {
        let done = row["download"]["bytes"].as_u64().unwrap_or(0) as f32;
        let total = row["download"]["total"].as_u64().unwrap_or(1).max(1) as f32;
        let progress = ui.add(egui::ProgressBar::new(done / total).show_percentage());
        register(ui.ctx(), format!("{prefix}:downloadProgress"), progress.rect);
        if text_button(ui, &format!("{prefix}:cancelDownload"), "Cancel download", false).clicked() {
            run(app, ui, "tags.cancelDownload", json!({"model":id}));
        }
    } else if row["installed"] != true || value["status"]["error"].is_string() {
        let label = if row["installed"] == true { "Download model again" } else { "Download model" };
        if text_button(ui, &format!("{prefix}:download"), label, false).clicked() {
            run(app, ui, "tags.download", json!({"model":id}));
        }
    }
    if let Some(error) = row["download"]["error"].as_str() {
        ui.label(crate::i18n::tr("Model download failed.")).on_hover_text(error);
    }
}

pub fn show(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(value) = model_rows(app, ui.ctx()) else {
        return;
    };
    if value["available"] != true {
        ui.label(crate::i18n::tr("Automatic keyword generation requires the desktop app. Existing tags remain searchable."));
        return;
    }
    ui.spacing_mut().item_spacing.y = 6.0;
    let current = app.session.tagging.settings.clone();
    let installed = ready(&value, &current);
    let total = value["coverage"]["total"].as_u64().unwrap_or(0);
    let tagged = value["coverage"]["tagged"].as_u64().unwrap_or(0);
    let missing = value["coverage"]["missing"].as_u64().unwrap_or(0);
    let count = ui.label(egui::RichText::new(format!("{tagged} / {total} {}", crate::i18n::tr("images tagged"))).color(t.text_label)).on_hover_text(
        crate::i18n::tr("Photos processed with the selected model in the current view, including photos with no confident suggestions."),
    );
    register(ui.ctx(), "tags:coverage", count.rect);
    let task = app.session.activity.list().into_iter().find(|t| t.kind == "tags");
    let running = task.is_some();
    let cancel = task.as_ref().filter(|t| t.cancellable && !t.cancelling).map(|t| t.id);
    if let Some(mut task) = task {
        task.done = tagged;
        task.total = total;
        let bar = super::activity::bar(ui, &t, &task, ui.available_width());
        register(ui.ctx(), "tags:progress", bar);
    }
    if running {
        let button = ui
            .add_enabled_ui(cancel.is_some(), |ui| text_button(ui, "tags:cancel", if cancel.is_some() { "Cancel" } else { "Stopping…" }, false))
            .inner;
        if button.clicked()
            && let Some(id) = cancel
        {
            run(app, ui, "activity.cancel", json!({"id":id}));
        }
    } else {
        let label = if missing > 0 { "Tag missing" } else { "Rescan" };
        let button = ui.add_enabled_ui(installed && total > 0 && !running, |ui| text_button(ui, "tags:scan", label, false)).inner;
        if button.clicked() {
            run(app, ui, "tags.scan", json!({"rescan":missing == 0}));
        }
        button.on_disabled_hover_text(crate::i18n::tr(if !installed {
            "Download a model in Settings > Auto Tagging first."
        } else if total == 0 {
            "No images in the current view."
        } else {
            "Tagging the current view."
        }));
    }
    let failed = value["status"]["failed"].as_u64().unwrap_or(0);
    if failed > 0 {
        ui.label(format!("{}: {failed}", crate::i18n::tr("Photos not tagged")))
            .on_hover_text(value["status"]["failures"].as_array().and_then(|a| a.first()).and_then(|v| v["error"].as_str()).unwrap_or(""));
    }
    if let Some(error) = value["status"]["error"].as_str() {
        ui.label(crate::i18n::tr("Tagging could not start.")).on_hover_text(error);
    }
    let Some(id) = app.session.active() else { return };
    let tags: Vec<_> = app.session.catalog.photo(id).map(|p| p.suggested_tags().cloned().collect()).unwrap_or_default();
    let has_suggestions = app.session.selected().iter().any(|id| app.session.catalog.photo(*id).is_some_and(|p| p.suggested_tags().next().is_some()));
    if has_suggestions {
        ui.add_space(6.0);
        crate::widgets::divider(ui);
        ui.label(egui::RichText::new(crate::i18n::tr("AI suggestions")).color(t.text_label))
            .on_hover_text(crate::i18n::tr("Already searchable. Save Metadata to File preserves AI tags separately in XMP. Accept adds ordinary keywords; dismiss excludes from search."));
    }
    for tag in tags {
        ui.horizontal_wrapped(|ui| {
            let label = format!("AI · {}", tag.name);
            let (body, cross) = super::keywording::chip(ui, &format!("ai:{}", tag.name), &label, false, true, ui.available_width());
            register(ui.ctx(), format!("tags:filter:{}", tag.name), body.rect);
            if body.on_hover_text(format!("{}\n{}\nClick to search this keyword.", tag.model, score_label(&tag.model, tag.score))).clicked() {
                run(app, ui, "library.filter", json!({"keyword":tag.name}));
            }
            if let Some(cross) = cross {
                register(ui.ctx(), format!("tags:dismiss:{}", tag.name), cross.rect);
                if cross.on_hover_text(crate::i18n::tr("Dismiss AI suggestion")).clicked() {
                    run(app, ui, "tags.review", json!({"ids":[id.0],"names":[tag.name],"action":"dismiss"}));
                }
            }
            if text_button(ui, &format!("tags:accept:{}", tag.name), "Accept", false).clicked() {
                run(app, ui, "tags.review", json!({"ids":[id.0],"names":[tag.name],"action":"accept"}));
            }
        });
    }
    if has_suggestions {
        ui.horizontal_wrapped(|ui| {
            if text_button(ui, "tags:acceptSelection", "Accept all", false)
                .on_hover_text(crate::i18n::tr("Accept suggestions for selected photos"))
                .clicked()
            {
                run(app, ui, "tags.review", json!({"action":"accept"}));
            }
            if text_button(ui, "tags:dismissSelection", "Dismiss all", false)
                .on_hover_text(crate::i18n::tr("Dismiss suggestions for selected photos"))
                .clicked()
            {
                run(app, ui, "tags.review", json!({"action":"dismiss"}));
            }
        });
    }
    let accepted: Vec<_> = app
        .session
        .catalog
        .photo(id)
        .map(|p| {
            p.meta
                .auto_tags
                .iter()
                .flat_map(|a| &a.tags)
                .filter(|t| t.accepted && p.meta.keywords.iter().any(|k| lightcraft_catalog::keywords::same(k, &t.name)))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if !accepted.is_empty() {
        let provenance = ui.menu_button(crate::i18n::tr("Accepted AI keywords"), |ui| {
            for tag in accepted {
                ui.label(&tag.name).on_hover_text(format!("{} · {}", tag.model, score_label(&tag.model, tag.score)));
            }
        });
        register(ui.ctx(), "tags:accepted", provenance.response.rect);
    }
}
