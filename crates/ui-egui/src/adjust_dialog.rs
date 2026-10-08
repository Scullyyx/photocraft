//! Modal Image › Adjustments dialogs (Levels, Curves, Hue/Saturation, …) built from the same
//! editors as the adjustment-layer Properties panel (`adjust_editors`).
//!
//! The dialog's fields are the command's complete parameter set (so automation can read and set
//! them with `ui.dialog.set`) plus private `__` keys. While it is open the canvas previews the
//! settings as a temporary adjustment layer clipped to the target (`adjust_preview`, composited on
//! the GPU); where that can't match the command, the *real command* runs on the proxy document
//! through the filter-preview machinery (`__filter` + `__preview`). OK runs the command once (one
//! history step) and Cancel drops the preview, leaving the document untouched.

use serde_json::{Map, Value, json};

use crate::PhotocraftApp;
use crate::adjust_editors::{self, EditorCx};
use crate::tone::{self, HistSource};

const PREFIX: &str = "image.adjustments.";

/// Whether `command` opens an adjustment dialog (when run without params).
pub fn has_dialog(command: &str) -> bool {
    command.strip_prefix(PREFIX).is_some_and(adjust_editors::has_editor)
}

pub fn owns(fields: &Map<String, Value>) -> bool {
    fields.contains_key("__adjust")
}

/// Root of the dialog editor's view state in egui memory.
fn dialog_mem(kind: &str) -> egui::Id {
    egui::Id::new(("adjust-dialog", kind))
}

/// Whether holding Alt turns the dialog's Cancel into Reset. Measured in Photoshop 25.4 for Color
/// Balance; other kinds keep Cancel until checked against Photoshop.
pub fn resets(fields: &Map<String, Value>) -> bool {
    fields.get("__adjust").and_then(Value::as_str) == Some("colorBalance")
}

/// Alt+Cancel (Reset): back to the defaults Photoshop's Reset restores, dialog stays open.
pub fn reset(ctx: &egui::Context, fields: &mut Map<String, Value>) {
    if !resets(fields) {
        return;
    }
    let mut values = crate::filter_dialog::params_of(fields);
    adjust_editors::reset_color_balance(ctx, dialog_mem("colorBalance"), &mut values);
    if let Value::Object(v) = values {
        fields.extend(v);
    }
}

/// Opens the dialog for `command` with the kind's neutral settings.
pub fn open(app: &mut PhotocraftApp, command: &str) -> Option<u64> {
    let kind = command.strip_prefix(PREFIX).filter(|k| adjust_editors::has_editor(k))?;
    let spec = photocraft_engine::commands::find(command)?;
    let mode = app.session.active().map_or(photocraft_doc::ColorMode::Rgb, |s| s.doc.mode);
    let defaults = photocraft_engine::adjust_params::default_for(kind, mode).ok()?;
    let mut fields = Map::new();
    fields.insert("__command".into(), json!(command));
    fields.insert("__label".into(), json!(spec.label));
    fields.insert("__adjust".into(), json!(kind));
    fields.insert("__filter".into(), json!(true));
    fields.insert("__preview".into(), json!(true));
    if let Value::Object(p) = photocraft_engine::adjust_params::to_params(&defaults) {
        fields.extend(p);
    }
    Some(app.ui.open_dialog(crate::state::DialogKind::Command, fields))
}

/// Dialog body: the kind's editor over the fields, then the Preview checkbox.
pub fn body(app: &mut PhotocraftApp, ui: &mut egui::Ui, fields: &mut Map<String, Value>) {
    let kind = fields.get("__adjust").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut values = crate::filter_dialog::params_of(fields);
    let active = app.session.active().and_then(|s| s.active_layer);
    let gray = app.session.active().is_some_and(|s| adjust_editors::is_gray(s.doc.mode));
    let hist = match active {
        Some(id) if adjust_editors::needs_histogram(&kind) => Some(tone::histograms(app, HistSource::Layer(id), adjust_editors::space_of(&values))),
        _ => None,
    };
    let cx = EditorCx { mem: dialog_mem(&kind), hist, gray, swatches: adjust_editors::swatches(app), dialog: true };
    let e = adjust_editors::editor(ui, &kind, &mut values, &cx);
    if e.changed
        && let Value::Object(v) = values
    {
        // Replace the parameter keys (an editor may remove one), keep the private ones.
        fields.retain(|k, _| k.starts_with("__"));
        fields.extend(v);
    }
    let mut preview = fields.get("__preview").and_then(Value::as_bool).unwrap_or(true);
    ui.add_space(6.0);
    if crate::widgets::checkbox(ui, &mut preview, tl!("Preview")).changed() {
        fields.insert("__preview".into(), json!(preview));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    fn app_with_image() -> PhotocraftApp {
        let mut s = photocraft_engine::Session::new();
        s.execute("file.new", json!({"width": 64, "height": 48})).unwrap();
        s.execute("select.rect", json!({"x": 0, "y": 0, "width": 32, "height": 48})).unwrap();
        s.execute("edit.fill", json!({"color": "#b04020"})).unwrap();
        s.execute("select.deselect", json!({})).unwrap();
        PhotocraftApp::new(s, crate::Services::default())
    }

    #[test]
    fn every_adjustment_with_an_editor_has_a_dialog() {
        for kind in adjust_editors::KINDS {
            assert!(has_dialog(&format!("image.adjustments.{kind}")), "{kind}");
        }
        for id in ["image.adjustments.invert", "image.adjustments.selectiveColor", "image.adjustments.desaturate", "filter.blur.gaussianBlur"] {
            assert!(!has_dialog(id), "{id}");
        }
    }

    /// Opens each dialog, renders it, changes a value, then cancels (document untouched) or
    /// confirms (one history step).
    #[test]
    fn dialogs_preview_cancel_and_commit() {
        for kind in adjust_editors::KINDS {
            let cmd = format!("image.adjustments.{kind}");
            let mut harness =
                Harness::builder().with_size(egui::vec2(1200.0, 900.0)).build_ui_state(|ui, app| crate::dialogs::show(app, ui.ctx()), app_with_image());
            PhotocraftApp::setup_context(&harness.ctx, crate::theme::ThemeKind::ALL[0]);
            let id = open(harness.state_mut(), &cmd).unwrap();
            harness.run_steps(3);
            let before = harness.state().session.active().unwrap().revision;
            let steps = harness.state().session.active().unwrap().history.past_len();
            // Change the settings the way automation does.
            let sample = photocraft_engine::adjust_params::to_params(
                &photocraft_engine::adjust_params::from_params(kind, &sample_params(kind), None, photocraft_doc::ColorMode::Rgb).unwrap(),
            );
            let d = harness.state_mut().ui.dialog_mut(id).unwrap();
            if let Value::Object(p) = sample {
                d.fields.extend(p);
            }
            harness.run_steps(2);
            // The preview runs the real command on the proxy.
            let fields = harness.state().ui.dialogs[0].fields.clone();
            let doc = harness.state().session.active().unwrap().doc.clone();
            let prev = crate::filter_dialog::preview_document(
                &doc,
                harness.state().session.active().unwrap().active_layer,
                &cmd,
                &crate::filter_dialog::params_of(&fields),
                1,
            );
            assert!(prev.is_some(), "{kind}: preview");
            assert_eq!(harness.state().session.active().unwrap().revision, before, "{kind}: previewing leaves the document alone");
            let r = crate::dialogs::confirm(harness.state_mut(), id);
            assert!(r.is_ok(), "{kind}: {r:?}");
            assert_eq!(harness.state().session.active().unwrap().history.past_len(), steps + 1, "{kind}: one history step");
        }
    }

    fn color_balance_dialog() -> (Harness<'static, PhotocraftApp>, u64) {
        // Real frame times: a double-click's two clicks must land within the double-click time.
        let mut h = Harness::builder()
            .with_size(egui::vec2(1200.0, 900.0))
            .with_step_dt(1.0 / 60.0)
            .build_ui_state(|ui, app| crate::dialogs::show(app, ui.ctx()), app_with_image());
        PhotocraftApp::setup_context(&h.ctx, crate::theme::ThemeKind::ALL[0]);
        let id = open(h.state_mut(), "image.adjustments.colorBalance").unwrap();
        let d = h.state_mut().ui.dialog_mut(id).unwrap();
        for (k, v) in
            [("shadows", json!([10, 0, 0])), ("midtones", json!([40, -30, 20])), ("highlights", json!([0, 0, -15])), ("preserveLuminosity", json!(false))]
        {
            d.fields.insert(k.into(), v);
        }
        h.run_steps(3);
        (h, id)
    }

    fn field(h: &Harness<'static, PhotocraftApp>, id: u64, key: &str) -> Value {
        h.state().ui.dialogs.iter().find(|d| d.id == id).map(|d| d.fields.get(key).cloned().unwrap_or(Value::Null)).unwrap_or(Value::Null)
    }

    fn input(h: &mut Harness<'static, PhotocraftApp>, mods: egui::Modifiers, events: &[egui::Event]) {
        h.event(egui::Event::ModifiersChanged(mods));
        h.run_steps(1);
        for e in events {
            h.event(e.clone());
            h.run_steps(1);
        }
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.run_steps(3);
    }

    /// Photoshop 25.4's Color Balance dialog (measured with the real application): a double-click
    /// on a slider zeroes it whatever the modifiers; the wheel over a slider steps it by 1, with
    /// Shift by 10 (Ctrl/Alt: 1); holding Alt turns Cancel into Reset, which zeroes every tone,
    /// shows Midtones, keeps Preserve Luminosity and leaves the dialog open.
    #[test]
    fn color_balance_dialog_gestures_match_photoshop() {
        use egui_kittest::kittest::Queryable;
        let (mut h, id) = color_balance_dialog();
        let label = h.query_all_by_label("Cyan  ·  Red").last().map(|n| n.rect()).expect("Cyan · Red row");
        let at = egui::pos2(label.left() + 60.0, label.bottom() + 14.0);
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::ALT };
        input(&mut h, egui::Modifiers::ALT, &[egui::Event::PointerMoved(at), press(true), press(false), press(true), press(false)]);
        assert_eq!(field(&h, id, "midtones"), json!([0.0, -30.0, 20.0]), "Alt+double-click zeroes the slider");
        assert_eq!(field(&h, id, "shadows"), json!([10, 0, 0]), "other tones untouched");
        let wheel =
            |n: f32, m| egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, n), phase: egui::TouchPhase::Move, modifiers: m };
        input(&mut h, egui::Modifiers::NONE, &[egui::Event::PointerMoved(at), wheel(3.0, egui::Modifiers::NONE)]);
        assert_eq!(field(&h, id, "midtones")[0], json!(3.0), "3 notches up");
        input(&mut h, egui::Modifiers::SHIFT, &[egui::Event::PointerMoved(at), wheel(2.0, egui::Modifiers::SHIFT)]);
        assert_eq!(field(&h, id, "midtones")[0], json!(23.0), "Shift: 10 per notch");
        input(&mut h, egui::Modifiers::CTRL, &[egui::Event::PointerMoved(at), wheel(-1.0, egui::Modifiers::CTRL)]);
        assert_eq!(field(&h, id, "midtones")[0], json!(22.0), "Ctrl: 1 per notch, down");
        // Alt turns Cancel into Reset.
        assert!(h.query_by_label("Cancel").is_some() && h.query_by_label("Reset").is_none());
        h.state_mut().ui.dialog_mut(id).unwrap().fields.insert("highlights".into(), json!([5, 5, 5]));
        let tone = egui::Id::new(("adjust-dialog", "colorBalance")).with("cb-tone");
        h.ctx.data_mut(|d| d.insert_temp(tone, 2usize));
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
        h.run_steps(2);
        let reset = h.get_by_label("Reset").rect();
        assert!(h.query_by_label("Cancel").is_none());
        let click =
            |pressed| egui::Event::PointerButton { pos: reset.center(), button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::ALT };
        input(&mut h, egui::Modifiers::ALT, &[egui::Event::PointerMoved(reset.center()), click(true), click(false)]);
        assert!(h.state().ui.dialogs.iter().any(|d| d.id == id), "Reset keeps the dialog open");
        for k in ["shadows", "midtones", "highlights"] {
            assert_eq!(field(&h, id, k), json!([0.0, 0.0, 0.0]), "{k}");
        }
        assert_eq!(field(&h, id, "preserveLuminosity"), json!(false), "Reset keeps Preserve Luminosity");
        assert_eq!(h.ctx.data(|d| d.get_temp::<usize>(tone)), Some(1), "Reset shows Midtones");
        // Without Alt it is Cancel again, and it closes.
        let cancel = h.get_by_label("Cancel").rect();
        let click =
            |pressed| egui::Event::PointerButton { pos: cancel.center(), button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        input(&mut h, egui::Modifiers::NONE, &[egui::Event::PointerMoved(cancel.center()), click(true), click(false)]);
        assert!(h.state().ui.dialogs.is_empty(), "Cancel closes");
    }

    /// Alt+Cancel is Reset only where it was checked against Photoshop.
    #[test]
    fn other_adjustment_dialogs_keep_cancel_under_alt() {
        use egui_kittest::kittest::Queryable;
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 900.0)).build_ui_state(|ui, app| crate::dialogs::show(app, ui.ctx()), app_with_image());
        PhotocraftApp::setup_context(&h.ctx, crate::theme::ThemeKind::ALL[0]);
        open(h.state_mut(), "image.adjustments.levels").unwrap();
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
        h.run_steps(3);
        assert!(h.query_by_label("Cancel").is_some() && h.query_by_label("Reset").is_none());
    }

    fn sample_params(kind: &str) -> Value {
        match kind {
            "brightnessContrast" => json!({"brightness": 40}),
            "levels" => json!({"inBlack": 30, "gamma": 1.3}),
            "curves" => json!({"points": [[0, 0], [100, 150], [255, 255]]}),
            "exposure" => json!({"exposure": 1}),
            "vibrance" => json!({"vibrance": 50}),
            "hueSaturation" => json!({"reds": {"hue": 40}}),
            "colorBalance" => json!({"midtones": [40, 0, -20]}),
            "blackWhite" => json!({"reds": 150, "tint": true}),
            "photoFilter" => json!({"filter": "cooling80", "density": 60}),
            "channelMixer" => json!({"red": [50, 50, 0, 0]}),
            "posterize" => json!({"levels": 3}),
            "threshold" => json!({"level": 90}),
            "gradientMap" => json!({"stops": [[0, "#200040"], [1, "#ffd080"]]}),
            _ => json!({}),
        }
    }

    #[test]
    fn cancel_leaves_the_document_untouched() {
        let mut harness =
            Harness::builder().with_size(egui::vec2(1200.0, 900.0)).build_ui_state(|ui, app| crate::dialogs::show(app, ui.ctx()), app_with_image());
        PhotocraftApp::setup_context(&harness.ctx, crate::theme::ThemeKind::ALL[0]);
        let id = open(harness.state_mut(), "image.adjustments.curves").unwrap();
        harness.run_steps(2);
        harness.state_mut().ui.dialog_mut(id).unwrap().fields.insert("points".into(), json!([[0, 255], [255, 0]]));
        harness.run_steps(2);
        let before = harness.state().session.active().unwrap().doc.clone();
        harness.state_mut().ui.close_dialog(id);
        harness.run_steps(2);
        let after = harness.state().session.active().unwrap().doc.clone();
        assert!(std::sync::Arc::ptr_eq(&before, &after));
        assert!(harness.state().ui.dialogs.is_empty());
    }
    #[test]
    fn curves_pointer_drag_previews_until_confirm_and_cancel_preserves_document() {
        for confirm in [false, true] {
            let mut h = Harness::builder().with_size(egui::vec2(1200.0, 900.0)).build_ui_state(|ui, app| crate::dialogs::show(app, ui.ctx()), app_with_image());
            PhotocraftApp::setup_context(&h.ctx, crate::theme::ThemeKind::ALL[0]);
            let original = h.state().session.active().unwrap().doc.clone();
            let steps = h.state().session.active().unwrap().history.past_len();
            let id = open(h.state_mut(), "image.adjustments.curves").unwrap();
            h.state_mut().ui.dialog_mut(id).unwrap().fields.insert("points".into(), json!([[0, 0], [128, 128], [255, 255]]));
            h.run_steps(4);
            let graph = h.ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("adjust-dialog", "curves")).with("curves-graph"))).unwrap();
            let a = egui::pos2(graph.left() + 128.0 / 255.0 * graph.width(), graph.bottom() - 128.0 / 255.0 * graph.height());
            h.hover_at(a);
            h.run_steps(1);
            h.drag_at(a);
            h.run_steps(1);
            h.hover_at(a + egui::vec2(40.0, -32.0));
            h.run_steps(1);
            h.drop_at(a + egui::vec2(40.0, -32.0));
            h.run_steps(3);
            let points = &h.state().ui.dialogs[0].fields["points"];
            assert_eq!(points.as_array().unwrap().len(), 3);
            assert!(points[1][0].as_f64().unwrap() > 150.0 && points[1][1].as_f64().unwrap() > 150.0, "{points}");
            assert!(std::sync::Arc::ptr_eq(&original, &h.state().session.active().unwrap().doc));
            assert_eq!(h.state().session.active().unwrap().history.past_len(), steps);
            if confirm {
                crate::dialogs::confirm(h.state_mut(), id).unwrap();
                assert_eq!(h.state().session.active().unwrap().history.past_len(), steps + 1);
                h.state_mut().run("edit.undo", json!({})).unwrap();
                assert!(std::sync::Arc::ptr_eq(&original, &h.state().session.active().unwrap().doc));
            } else {
                h.state_mut().ui.close_dialog(id);
                h.run_steps(2);
                assert!(std::sync::Arc::ptr_eq(&original, &h.state().session.active().unwrap().doc));
            }
        }
    }
}
