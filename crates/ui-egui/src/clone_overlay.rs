//! Clone Stamp source preview: once a source point is set (⌥-click), the brush under the pointer
//! shows the pixels a stroke there would clone, shaped and faded like one dab of the brush (its
//! size, hardness and tip). The first stroke ends the preview; setting a new source brings it
//! back (`UiState::clone_overlay`).

use egui::{Color32, Pos2, Rect, pos2};
use photocraft_engine::paint::StrokePoint;

use crate::PhotocraftApp;
use crate::canvas::ViewXform;
use crate::state::Tool;

/// The cached preview: what it was rendered for, and its texture over `rect` (document px,
/// centred on the source point).
#[derive(Clone)]
struct Cached {
    key: String,
    rect: photocraft_geom::Rect,
    tex: egui::TextureHandle,
}

/// Draw the preview under the pointer at `doc` (document px), when the Clone Stamp has a fresh
/// source.
pub fn draw(app: &PhotocraftApp, painter: &egui::Painter, xf: &ViewXform, doc: [f64; 2]) {
    if app.ui.tool != Tool::CloneStamp || !app.ui.clone_overlay || app.drag.is_some() {
        return;
    }
    let Some(src) = crate::retouch_ui::sample_point(app, Some(doc)) else { return };
    let Some(st) = app.session.active() else { return };
    let brush = &app.session.tools.brush;
    let ctx = painter.ctx();
    let key = format!("{:?}/{}/{:.0},{:.0}/{:?}", st.doc.id, st.revision, src[0], src[1], brush);
    let id = egui::Id::new("clone-overlay");
    let cached = ctx.data(|d| d.get_temp::<Cached>(id)).filter(|c| c.key == key);
    let cached = match cached {
        Some(c) => c,
        None => {
            let Some(c) = render(ctx, &st.doc, brush, src, key) else { return };
            ctx.data_mut(|d| d.insert_temp(id, c.clone()));
            c
        }
    };
    // The source region, moved so the source point sits under the pointer.
    let (dx, dy) = ((doc[0] - src[0]) as f32, (doc[1] - src[1]) as f32);
    let r = cached.rect;
    let a = xf.to_screen(r.x0 as f32 + dx, r.y0 as f32 + dy);
    let b = xf.to_screen(r.x1 as f32 + dx, r.y1 as f32 + dy);
    let uv = if xf.flip { Rect::from_min_max(pos2(1.0, 0.0), pos2(0.0, 1.0)) } else { Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)) };
    painter.image(cached.tex.id(), Rect::from_two_pos(a, b), uv, Color32::WHITE);
}

/// One dab of `brush` centred on `src`, filled with the document's pixels there.
fn render(ctx: &egui::Context, doc: &photocraft_doc::Document, brush: &photocraft_engine::paint::BrushSettings, src: [f64; 2], key: String) -> Option<Cached> {
    let mut dab = photocraft_engine::paint::StrokeRenderer::new(brush, None, 1.0);
    dab.push(&[StrokePoint::new(src[0], src[1], 1.0)]);
    dab.finish();
    let rect = dab.bounds();
    if rect.is_empty() || rect.width() > 4096 || rect.height() > 4096 {
        return None;
    }
    let cov = dab.coverage_in(rect);
    let px = photocraft_compose::render(doc, rect).to_rgba8();
    let opacity = brush.opacity.clamp(0.0, 1.0);
    let mut rgba = px.pixels;
    for (i, c) in cov.iter().enumerate() {
        let a = &mut rgba[i * 4 + 3];
        *a = (f32::from(*a) * (c * opacity).clamp(0.0, 1.0)).round() as u8;
    }
    let img = egui::ColorImage::from_rgba_unmultiplied([rect.width() as usize, rect.height() as usize], &rgba);
    let tex = ctx.load_texture("clone-overlay", img, egui::TextureOptions::LINEAR);
    Some(Cached { key, rect, tex })
}
