//! Preview swatches and header details for LUT rows, produced off the UI thread and cached in egui
//! memory. Only rows that are drawn ask for anything, and at most a few files are read at once, so
//! a pack of hundreds of LUTs fills in as you scroll.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use egui::{ColorImage, Context, Id, TextureHandle, TextureOptions};
use photocraft_engine::lut_library::meta::{self, Header};
use photocraft_engine::lut_library::thumb::{self, THUMB_H, THUMB_W};

/// Files read for previews at the same time.
const MAX_INFLIGHT: usize = 3;
/// Headers read per frame.
const HEADERS_PER_FRAME: usize = 24;

enum Slot {
    Pending,
    Ready(TextureHandle),
    Failed,
}

#[derive(Default)]
struct Inner {
    slots: HashMap<String, Slot>,
    done: Vec<(String, Result<Vec<u8>, String>)>,
    inflight: usize,
    headers: HashMap<String, Option<Header>>,
    /// Library revision the duplicate scan last started for, and its result.
    scan_rev: Option<u64>,
    redundant: HashMap<String, String>,
}

/// The preview and header caches (shared with the worker threads).
#[derive(Clone, Default)]
pub(super) struct Thumbs(Arc<Mutex<Inner>>);

impl Thumbs {
    pub(super) fn get(ctx: &Context) -> Thumbs {
        let id = Id::new("lutlib-thumbs");
        if let Some(t) = ctx.data(|d| d.get_temp::<Thumbs>(id)) {
            return t;
        }
        let t = Thumbs::default();
        ctx.data_mut(|d| d.insert_temp(id, t.clone()));
        t
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Turn finished previews into textures. Call once per frame.
    pub(super) fn collect(&self, ctx: &Context) {
        let done = std::mem::take(&mut self.inner().done);
        for (key, result) in done {
            let slot = match result {
                Ok(rgba) => {
                    Slot::Ready(ctx.load_texture(format!("lut-{key}"), ColorImage::from_rgba_unmultiplied([THUMB_W, THUMB_H], &rgba), TextureOptions::LINEAR))
                }
                Err(_) => Slot::Failed,
            };
            self.inner().slots.insert(key, slot);
        }
    }

    /// The preview for `key`, starting its render when nobody has yet. `source` is the LUT file, or
    /// `None` for the built-in look named by `key` (`look:<id>`).
    pub(super) fn texture(&self, ctx: &Context, key: &str, source: Option<PathBuf>) -> Option<TextureHandle> {
        let mut inner = self.inner();
        match inner.slots.get(key) {
            Some(Slot::Ready(t)) => return Some(t.clone()),
            Some(Slot::Pending | Slot::Failed) => return None,
            None => {}
        }
        if inner.inflight >= MAX_INFLIGHT {
            return None;
        }
        inner.inflight += 1;
        inner.slots.insert(key.to_string(), Slot::Pending);
        drop(inner);
        let (shared, ctx, key) = (self.clone(), ctx.clone(), key.to_string());
        let work = move || {
            let result = match &source {
                Some(path) => thumb::render_file(path),
                None => key.strip_prefix("look:").and_then(thumb::render_builtin).ok_or_else(|| "unknown look".to_string()),
            };
            let mut inner = shared.inner();
            inner.inflight = inner.inflight.saturating_sub(1);
            inner.done.push((key, result));
            drop(inner);
            ctx.request_repaint();
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(work);
        #[cfg(target_arch = "wasm32")]
        work();
        None
    }

    /// The header of the LUT file at `path` (read on the first request, a few per frame).
    pub(super) fn header(&self, path: &str, budget: &mut usize) -> Option<Header> {
        let mut inner = self.inner();
        if let Some(h) = inner.headers.get(path) {
            return h.clone();
        }
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let h = meta::read_header(std::path::Path::new(path));
        inner.headers.insert(path.to_string(), h.clone());
        h
    }

    /// Packs that another pack already contains (`pack → the pack that covers it`). The scan reads
    /// every LUT the first time, so it runs on a thread and the answer arrives a moment later.
    pub(super) fn redundant(&self, ctx: &Context, root: &std::path::Path, rev: u64) -> HashMap<String, String> {
        let mut inner = self.inner();
        if inner.scan_rev != Some(rev) {
            inner.scan_rev = Some(rev);
            let (shared, ctx, root) = (self.clone(), ctx.clone(), root.to_path_buf());
            let work = move || {
                let found = photocraft_engine::lut_library::LutLibrary::new(root).redundant_packs();
                let mut inner = shared.inner();
                if inner.scan_rev == Some(rev) {
                    inner.redundant = found;
                }
                drop(inner);
                ctx.request_repaint();
            };
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::spawn(work);
            #[cfg(target_arch = "wasm32")]
            work();
        }
        inner.redundant.clone()
    }

    pub(super) fn header_budget() -> usize {
        HEADERS_PER_FRAME
    }
}
