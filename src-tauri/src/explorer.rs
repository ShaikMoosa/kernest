//! File Explorer integration for `.kernest` fonts (S70).
//!
//! A `.kernest` font is a folder, and Explorer has no thumbnail or preview handlers
//! for folders. It does read a folder's `desktop.ini`: so on save Kernest draws "Aa"
//! in the font itself as the folder's icon, and sets its tooltip to a summary such as
//! "Kernest Sans · 3 masters · 412 glyphs · 7 instances". The right-click actions
//! (Open, Export fonts here, Check font) come from the installer.

use std::path::Path;

use kernest_core::Font;

const ICON_FILE: &str = "kernest-preview.ico";
const ICON_SIZE: usize = 256;

/// Gives a saved `.kernest` folder its Explorer icon and tooltip. Failures are
/// ignored: this is a nicety, never a reason for a save to fail.
pub fn decorate(dir: &Path, font: &Font) {
    let _ = try_decorate(dir, font);
}

fn try_decorate(dir: &Path, font: &Font) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    if let Some(rgba) = render_sample(font, ICON_SIZE) {
        let png = encode_png(&rgba, ICON_SIZE as u32, ICON_SIZE as u32)?;
        std::fs::write(dir.join(ICON_FILE), ico(&png)).map_err(|e| e.to_string())?;
    }
    let ini = format!(
        "[.ShellClassInfo]\r\nIconResource={ICON_FILE},0\r\nInfoTip={}\r\n",
        summary(font)
    );
    // UTF-16 with a byte-order mark, so family names in any script show correctly.
    let mut bytes = vec![0xFF, 0xFE];
    for u in ini.encode_utf16() {
        bytes.extend(u.to_le_bytes());
    }
    let ini_path = dir.join("desktop.ini");
    platform::unhide(&ini_path);
    std::fs::write(&ini_path, bytes).map_err(|e| e.to_string())?;
    platform::mark(dir, &ini_path, &dir.join(ICON_FILE));
    Ok(())
}

/// "Kernest Sans · 3 masters · 412 glyphs · 7 instances"
pub fn summary(font: &Font) -> String {
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut parts = vec![
        font.info.family_name.clone(),
        plural(font.masters.len(), "master", "masters"),
        plural(font.glyphs.len(), "glyph", "glyphs"),
    ];
    if !font.instances.is_empty() {
        parts.push(plural(font.instances.len(), "instance", "instances"));
    }
    parts.join(" · ")
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_SYSTEM, INVALID_FILE_ATTRIBUTES,
    };
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_UPDATEITEM, SHCNF_PATHW};

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Lets a hidden, system desktop.ini be overwritten.
    pub fn unhide(file: &Path) {
        if file.exists() {
            let w = wide(file);
            unsafe { SetFileAttributesW(w.as_ptr(), FILE_ATTRIBUTE_NORMAL) };
        }
    }

    /// Explorer reads desktop.ini only in read-only (or system) folders.
    pub fn mark(dir: &Path, ini: &Path, icon: &Path) {
        unsafe {
            let d = wide(dir);
            let a = GetFileAttributesW(d.as_ptr());
            if a != INVALID_FILE_ATTRIBUTES {
                SetFileAttributesW(d.as_ptr(), a | FILE_ATTRIBUTE_READONLY);
            }
            for f in [ini, icon] {
                if f.exists() {
                    let w = wide(f);
                    SetFileAttributesW(w.as_ptr(), FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM);
                }
            }
            SHChangeNotify(
                SHCNE_UPDATEITEM as i32,
                SHCNF_PATHW,
                d.as_ptr().cast(),
                std::ptr::null(),
            );
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;
    pub fn unhide(_: &Path) {}
    pub fn mark(_: &Path, _: &Path, _: &Path) {}
}

// --- Drawing ------------------------------------------------------------------------------

type Contour = Vec<(f64, f64)>;

/// The glyphs to draw: "Aa" if the font has them, else the first two drawn glyphs.
fn sample_glyphs(font: &Font) -> Vec<String> {
    let master = &kernest_core::interpolate::default_master(font).id;
    let drawn = |name: &str| {
        font.glyph(name)
            .and_then(|g| g.layer(master))
            .is_some_and(|l| !l.shapes.is_empty())
    };
    for pair in [["A", "a"], ["H", "o"], ["a", "g"]] {
        if pair.iter().all(|n| drawn(n)) {
            return pair.iter().map(|s| s.to_string()).collect();
        }
    }
    font.glyphs
        .iter()
        .filter(|g| g.export && drawn(&g.name))
        .take(2)
        .map(|g| g.name.clone())
        .collect()
}

fn contours(d: &str) -> Vec<Contour> {
    let mut out: Vec<Contour> = Vec::new();
    let mut cur: Contour = Vec::new();
    let mut nums: Vec<f64> = Vec::new();
    let mut cmd = ' ';
    let flush = |cmd: char, nums: &mut Vec<f64>, cur: &mut Contour, out: &mut Vec<Contour>| {
        let last = cur.last().copied().unwrap_or((0.0, 0.0));
        match cmd {
            'M' if nums.len() >= 2 => {
                if cur.len() > 2 {
                    out.push(std::mem::take(cur));
                }
                cur.clear();
                cur.push((nums[0], nums[1]));
            }
            'L' if nums.len() >= 2 => cur.push((nums[0], nums[1])),
            'Q' if nums.len() >= 4 => {
                let (c, e) = ((nums[0], nums[1]), (nums[2], nums[3]));
                for i in 1..=12 {
                    let t = i as f64 / 12.0;
                    let u = 1.0 - t;
                    cur.push((
                        u * u * last.0 + 2.0 * u * t * c.0 + t * t * e.0,
                        u * u * last.1 + 2.0 * u * t * c.1 + t * t * e.1,
                    ));
                }
            }
            'C' if nums.len() >= 6 => {
                let (a, b, e) = ((nums[0], nums[1]), (nums[2], nums[3]), (nums[4], nums[5]));
                for i in 1..=16 {
                    let t = i as f64 / 16.0;
                    let u = 1.0 - t;
                    cur.push((
                        u * u * u * last.0
                            + 3.0 * u * u * t * a.0
                            + 3.0 * u * t * t * b.0
                            + t * t * t * e.0,
                        u * u * u * last.1
                            + 3.0 * u * u * t * a.1
                            + 3.0 * u * t * t * b.1
                            + t * t * t * e.1,
                    ));
                }
            }
            'Z' => {
                if cur.len() > 2 {
                    out.push(std::mem::take(cur));
                }
                cur.clear();
            }
            _ => {}
        }
        nums.clear();
    };
    let mut token = String::new();
    for ch in d.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphabetic() {
            if !token.is_empty() {
                nums.push(token.parse().unwrap_or(0.0));
                token.clear();
            }
            if cmd != ' ' {
                flush(cmd, &mut nums, &mut cur, &mut out);
            }
            cmd = ch;
            if ch == 'Z' {
                flush('Z', &mut nums, &mut cur, &mut out);
                cmd = ' ';
            }
        } else if ch == ' ' || ch == ',' {
            if !token.is_empty() {
                nums.push(token.parse().unwrap_or(0.0));
                token.clear();
            }
        } else {
            token.push(ch);
        }
    }
    if cmd != ' ' {
        flush(cmd, &mut nums, &mut cur, &mut out);
    }
    if cur.len() > 2 {
        out.push(cur);
    }
    out
}

/// "Aa" drawn in the font on a white tile, as RGBA rows (top first).
pub fn render_sample(font: &Font, size: usize) -> Option<Vec<u8>> {
    let names = sample_glyphs(font);
    if names.is_empty() {
        return None;
    }
    let master = kernest_core::interpolate::default_master(font);
    let mut shapes: Vec<Contour> = Vec::new();
    let mut x = 0.0;
    for n in &names {
        let l = font.glyph(n)?.layer(&master.id)?;
        for c in contours(&kernest_core::geometry::svg_path(font, l)) {
            shapes.push(c.into_iter().map(|(px, py)| (px + x, py)).collect());
        }
        x += l.width;
    }
    let (asc, desc) = (master.metrics.ascender, master.metrics.descender);
    let (min_x, max_x) = shapes
        .iter()
        .flatten()
        .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    if min_x >= max_x || asc <= desc {
        return None;
    }
    let pad = size as f64 * 0.14;
    let avail = size as f64 - 2.0 * pad;
    let s = (avail / (max_x - min_x)).min(avail / (asc - desc));
    let ox = (size as f64 - (max_x - min_x) * s) / 2.0 - min_x * s;
    let oy = (size as f64 + (asc + desc) * s) / 2.0; // baseline, y down
    let px: Vec<Contour> = shapes
        .iter()
        .map(|c| c.iter().map(|&(x, y)| (ox + x * s, oy - y * s)).collect())
        .collect();
    let cover = coverage(&px, size);
    let mut rgba = vec![0u8; size * size * 4];
    let radius = size as f64 * 0.12;
    let ink = [11u8, 13, 23];
    for y in 0..size {
        for x in 0..size {
            // Rounded white tile.
            let inside = {
                let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
                let cx = fx.clamp(radius, size as f64 - radius);
                let cy = fy.clamp(radius, size as f64 - radius);
                ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() <= radius
            };
            if !inside {
                continue;
            }
            let a = cover[y * size + x];
            let i = (y * size + x) * 4;
            for k in 0..3 {
                rgba[i + k] = (255.0 * (1.0 - a) + ink[k] as f64 * a).round() as u8;
            }
            rgba[i + 3] = 255;
        }
    }
    Some(rgba)
}

/// Fill coverage (0–1) per pixel, nonzero winding, 4×4 samples per pixel.
fn coverage(contours: &[Contour], size: usize) -> Vec<f64> {
    const SS: usize = 4;
    let mut out = vec![0.0f64; size * size];
    let mut edges: Vec<(f64, f64, f64, f64, i32)> = Vec::new();
    for c in contours {
        for i in 0..c.len() {
            let (a, b) = (c[i], c[(i + 1) % c.len()]);
            if (a.1 - b.1).abs() < 1e-12 {
                continue;
            }
            let dir = if b.1 > a.1 { 1 } else { -1 };
            edges.push((a.0, a.1, b.0, b.1, dir));
        }
    }
    let mut xs: Vec<(f64, i32)> = Vec::new();
    for sy in 0..size * SS {
        let y = (sy as f64 + 0.5) / SS as f64;
        xs.clear();
        for &(x0, y0, x1, y1, d) in &edges {
            let (lo, hi) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            if y >= lo && y < hi {
                xs.push((x0 + (y - y0) * (x1 - x0) / (y1 - y0), d));
            }
        }
        xs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let row = sy / SS;
        let mut wind = 0;
        for w in 0..xs.len() {
            let before = wind;
            wind += xs[w].1;
            if before == 0 && wind != 0 {
                let start = xs[w].0;
                let mut end = start;
                let mut k = wind;
                for v in xs.iter().skip(w + 1) {
                    k += v.1;
                    if k == 0 {
                        end = v.0;
                        break;
                    }
                }
                for sx in 0..size * SS {
                    let x = (sx as f64 + 0.5) / SS as f64;
                    if x >= start && x < end {
                        out[row * size + sx / SS] += 1.0 / (SS * SS) as f64;
                    }
                }
            }
        }
    }
    for v in &mut out {
        *v = v.min(1.0);
    }
    out
}

fn encode_png(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// A one-image .ico holding a 256×256 PNG.
fn ico(png: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(22 + png.len());
    out.extend([0, 0, 1, 0, 1, 0]); // reserved, type 1 (icon), one image
    out.extend([0, 0, 0, 0]); // width 256, height 256 (as 0), no palette, reserved
    out.extend(1u16.to_le_bytes()); // colour planes
    out.extend(32u16.to_le_bytes()); // bits per pixel
    out.extend((png.len() as u32).to_le_bytes());
    out.extend(22u32.to_le_bytes()); // image data offset
    out.extend(png);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_the_sample_font() {
        let f = kernest_core::sample::sample_font();
        assert_eq!(sample_glyphs(&f).len(), 2);
        if let Some(out) = std::env::var_os("KERNEST_ICON_OUT") {
            let big = render_sample(&f, 256).unwrap();
            std::fs::write(out, encode_png(&big, 256, 256).unwrap()).unwrap();
        }
        let rgba = render_sample(&f, 64).unwrap();
        let dark = rgba.chunks(4).filter(|p| p[3] == 255 && p[0] < 100).count();
        assert!(dark > 100, "ink drawn: {dark}");
        let png = encode_png(&rgba, 64, 64).unwrap();
        assert_eq!(&png[..4], b"\x89PNG");
        let i = ico(&png);
        assert_eq!(&i[..6], &[0, 0, 1, 0, 1, 0]);
        assert!(summary(&f).contains("glyphs"));
    }

    #[test]
    fn parses_path_data() {
        let c = contours("M0 0L10 0L10 10Z M20 20Q30 20 30 30L20 30Z");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].len(), 3);
        assert!(c[1].len() > 3);
    }
}
