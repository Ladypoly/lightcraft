//! Batch Reload from Disk uses the import probes and shared cancellable activity stack.
use lightcraft_engine::{activity::Cancel, cmd::convert};
use serde_json::{Value, json};

use crate::LightcraftApp;

pub const LABEL: &str = "Reloading metadata";

pub fn start(app: &mut LightcraftApp, params: &Value) -> Result<Value, String> {
    if app.tasks.is_running(LABEL) {
        return Err(crate::i18n::tr("A metadata reload is already running.").into());
    }
    let input = convert::prepare_reload(&app.session, params).map_err(|e| e.to_string())?;
    let total = input.total();
    let library = app.session.library_generation();
    let task = app.session.activity.start("reload", LABEL, Cancel::Yes);
    task.progress(0, total as u64);
    let work = move || (input.probe(Some(&task)), task);
    let done = move |app: &mut LightcraftApp, ctx: &egui::Context, (results, task): (_, lightcraft_engine::activity::TaskGuard)| {
        if app.session.library_generation() != library {
            return;
        }
        if task.is_cancelled() {
            app.toast(ctx, crate::i18n::tr("Metadata reload cancelled."));
            return;
        }
        match app.session.execute_fn("photo.reload", |s| convert::apply_reload(s, results)) {
            Ok(report) => {
                let n = report["reloaded"].as_array().map_or(0, Vec::len);
                let failed = report["failed"].as_array().map_or(0, Vec::len);
                let skipped = report["skipped"].as_u64().unwrap_or(0);
                app.toast(
                    ctx,
                    format!(
                        "{}: {n} · {}: {failed} · {}: {skipped}",
                        crate::i18n::tr("Metadata refreshed"),
                        crate::i18n::tr("Unreadable"),
                        crate::i18n::tr("Changed meanwhile")
                    ),
                );
            }
            Err(e) => app.toast_error(ctx, e.to_string()),
        }
    };
    crate::tasks::spawn(app, LABEL, None, work, done)?;
    Ok(json!({"started":true,"total":total}))
}
