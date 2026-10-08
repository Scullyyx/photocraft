//! Small previews of what a LUT does: a built-in sample picture (a hue sweep fading to dark and a
//! grey ramp, generated here, so there is nothing to license) put through the LUT.

use std::fs;
use std::path::Path;

use photocraft_cms::lutfile::{self, LutFile};

use super::MAX_LUT_BYTES;

/// Preview size in pixels.
pub const THUMB_W: usize = 40;
pub const THUMB_H: usize = 24;

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h6 = (h.rem_euclid(1.0)) * 6.0;
    let (c, i) = (v * s, h6.floor());
    let x = c * (1.0 - ((h6 % 2.0) - 1.0).abs());
    let (r, g, b) = match i as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

/// The sample picture, `w × h` linear-light-agnostic RGB in 0..=1: colours across, lighter to
/// darker down the top three quarters, and a grey ramp along the bottom.
pub fn sample(w: usize, h: usize) -> Vec<[f32; 3]> {
    let ramp_top = h * 3 / 4;
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let u = x as f32 / w.max(1) as f32;
            px.push(if y >= ramp_top {
                [u, u, u]
            } else {
                let t = y as f32 / ramp_top.max(1) as f32;
                hsv(u * 0.92, 0.75 - 0.35 * (1.0 - t), 1.0 - 0.7 * t)
            });
        }
    }
    px
}

/// Trilinear lookup of `rgb` (0..=1) in `lut`.
pub fn apply(lut: &LutFile, rgb: [f32; 3]) -> [f32; 3] {
    let n = lut.size;
    if n < 2 {
        return rgb;
    }
    let scale = (n - 1) as f32;
    let pos: Vec<(usize, usize, f32)> = rgb
        .iter()
        .map(|c| {
            let p = c.clamp(0.0, 1.0) * scale;
            let lo = (p.floor() as usize).min(n - 2);
            (lo, lo + 1, p - lo as f32)
        })
        .collect();
    let (Some(&(r0, r1, rf)), Some(&(g0, g1, gf)), Some(&(b0, b1, bf))) = (pos.first(), pos.get(1), pos.get(2)) else { return rgb };
    let at = |r: usize, g: usize, b: usize, c: usize| lut.data.get(((b * n + g) * n + r) * 3 + c).copied().unwrap_or(0.0);
    let mut out = [0.0f32; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let c00 = lerp(at(r0, g0, b0, c), at(r1, g0, b0, c), rf);
        let c10 = lerp(at(r0, g1, b0, c), at(r1, g1, b0, c), rf);
        let c01 = lerp(at(r0, g0, b1, c), at(r1, g0, b1, c), rf);
        let c11 = lerp(at(r0, g1, b1, c), at(r1, g1, b1, c), rf);
        *o = lerp(lerp(c00, c10, gf), lerp(c01, c11, gf), bf).clamp(0.0, 1.0);
    }
    out
}

/// The sample picture through `lut`, as `THUMB_W × THUMB_H` RGBA8 bytes.
pub fn render(lut: &LutFile) -> Vec<u8> {
    sample(THUMB_W, THUMB_H)
        .into_iter()
        .flat_map(|p| {
            let o = apply(lut, p);
            [(o[0] * 255.0 + 0.5) as u8, (o[1] * 255.0 + 0.5) as u8, (o[2] * 255.0 + 0.5) as u8, 255]
        })
        .collect()
}

/// A preview of the LUT file at `path`.
pub fn render_file(path: &Path) -> Result<Vec<u8>, String> {
    let len = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if len > MAX_LUT_BYTES {
        return Err(format!("{}: file too large", path.display()));
    }
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let lut = lutfile::parse(&path.to_string_lossy(), &bytes).map_err(|e| e.0)?;
    Ok(render(&lut))
}

/// A preview of the built-in look `id`.
pub fn render_builtin(id: &str) -> Option<Vec<u8>> {
    lutfile::builtin(id).map(|l| render(&l))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(rgba: &[u8], i: usize) -> [u8; 3] {
        [rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2]]
    }

    #[test]
    fn an_identity_lut_leaves_the_sample_alone() {
        let id = render(&LutFile::identity(9));
        let want = sample(THUMB_W, THUMB_H);
        assert_eq!(id.len(), THUMB_W * THUMB_H * 4);
        for (i, p) in want.iter().enumerate() {
            let got = px(&id, i);
            for c in 0..3 {
                assert!((f32::from(got[c]) - p[c] * 255.0).abs() <= 1.5, "pixel {i} channel {c}");
            }
        }
    }

    #[test]
    fn an_inverting_lut_inverts_the_sample() {
        let inv = LutFile::from_fn("Invert", 9, |c| [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]]);
        let (out, base) = (render(&inv), render(&LutFile::identity(9)));
        for i in 0..THUMB_W * THUMB_H {
            let (a, b) = (px(&out, i), px(&base, i));
            for c in 0..3 {
                assert!((i32::from(a[c]) + i32::from(b[c]) - 255).abs() <= 3, "pixel {i}");
            }
        }
    }

    #[test]
    fn broken_input_is_an_error_not_a_panic() {
        assert!(render_file(&std::env::temp_dir().join("photocraft-thumb-missing.cube")).is_err());
        assert!(render_builtin("nope").is_none());
        assert!(render_builtin("warm").is_some());
        assert_eq!(apply(&LutFile { title: String::new(), size: 1, data: vec![] }, [0.2, 0.3, 0.4]), [0.2, 0.3, 0.4]);
        let short = LutFile { title: String::new(), size: 3, data: vec![0.5; 4] };
        assert!(apply(&short, [0.1, 0.2, 0.3]).iter().all(|v| v.is_finite()), "a truncated table does not panic");
    }
}
