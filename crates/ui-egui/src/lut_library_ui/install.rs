//! Installing LUT packs from the UI: the folder dialog, drops, and the status line, plus the
//! housekeeping that ends a canvas preview when the browser stops being drawn.

use std::path::Path;

use egui::Id;
use serde_json::{Value, json};

use crate::PhotocraftApp;

/// The status line after `lut.installPack` finished (`v` is its result).
pub fn report(app: &mut PhotocraftApp, v: &Value) {
    let n = v.get("count").and_then(Value::as_u64).unwrap_or(0);
    let pack = v.get("pack").and_then(Value::as_str).unwrap_or_default();
    let skipped = v.get("skipped").and_then(Value::as_array).map_or(0, Vec::len);
    let dupes = v.get("duplicates").and_then(Value::as_array).map_or(0, Vec::len);
    let mut line = format!("Installed {n} LUTs into {pack}");
    let notes: Vec<String> =
        [(skipped, "skipped"), (dupes, "already installed")].iter().filter(|(c, _)| *c > 0).map(|(c, what)| format!("{c} {what}")).collect();
    if !notes.is_empty() {
        line.push_str(&format!(" ({})", notes.join(", ")));
    }
    app.ui.status = line;
    app.ui.status_error = false;
}

/// Install the folder or `.zip` at `path` as a pack, reporting the result in the status bar.
pub fn install_path(app: &mut PhotocraftApp, path: &str) {
    if let Ok(v) = app.run("lut.installPack", json!({"path": path}))
        && v.get("count").is_some()
    {
        report(app, &v);
    }
}

/// Whether a dropped file is a LUT pack (a folder or a `.zip`) and this session has a library.
pub fn is_pack_drop(app: &PhotocraftApp, path: &Path) -> bool {
    app.session.lut_library.is_some() && path.is_absolute() && (path.is_dir() || path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")))
}

pub(super) fn install_dialog(app: &mut PhotocraftApp) {
    let Some(pick) = app.services.pick_folder.as_mut() else {
        app.ui.status = "Choosing a folder is not available here; use the lut.installPack command".into();
        app.ui.status_error = true;
        return;
    };
    if let Some(dir) = pick() {
        install_path(app, &dir);
    }
}

/// The preview of a LUT stops when the editor is no longer drawn (another layer, a closed panel).
pub fn end_stale_preview(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let (flag, seen): (u64, u64) = ctx.data(|d| d.get_temp(Id::new("lutlib-previewing"))).unwrap_or_default();
    if flag != 0 && ctx.cumulative_frame_nr() > seen + 1 {
        if app.live_adjust.as_ref().is_some_and(|(l, _)| l.0 == flag) {
            app.live_adjust = None;
        }
        ctx.data_mut(|d| d.insert_temp(Id::new("lutlib-previewing"), (0u64, 0u64)));
    }
}
