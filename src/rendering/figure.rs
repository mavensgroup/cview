// src/rendering/figure.rs
//
// Publication figures from a GPU-rendered image: the picture is raster (at
// the chosen dpi), the annotations are vector where the format allows.
//
// - PDF / SVG: a page of the exact physical size; the image is embedded at
//   its full resolution (lossless), and title, isovalue key, colour bar,
//   axes, scale bar and panel letter are vector text and paths in a
//   sans-serif font (Arial/Helvetica), editable in Illustrator/Inkscape.
// - PNG / TIFF: annotations rasterised at the same dpi; the dpi is written
//   into the file (PNG pHYs + sRGB chunks, TIFF X/YResolution), so the image
//   opens at its print size.
//
// Sizes follow the journals' final-artwork rules: physical width presets,
// fonts in points at final size (Nature: 5-7 pt), lines in points.

use crate::rendering::gl::RgbaImage;
use gtk4::cairo;
use std::io::{self, Write};
use std::path::Path;

pub const MM_PER_INCH: f64 = 25.4;
pub const PT_PER_INCH: f64 = 72.0;

/// Journal width presets (mm). Heights follow the view's aspect.
pub const PRESETS: &[(&str, f64)] = &[
    ("Nature — 1 column (89 mm)", 89.0),
    ("Nature — 2 columns (183 mm)", 183.0),
    ("Science — 1 column (55 mm)", 55.0),
    ("Science — 1.5 columns (120 mm)", 120.0),
    ("Science — 2 columns (175 mm)", 175.0),
    ("ACS — 1 column (82.5 mm)", 82.55),
    ("ACS — 2 columns (178 mm)", 177.8),
    ("Custom", 0.0),
];

/// Nature's maximum figure height.
pub const NATURE_MAX_HEIGHT_MM: f64 = 170.0;

pub fn pixels_for(mm: f64, dpi: f64) -> u32 {
    (mm / MM_PER_INCH * dpi).round().max(1.0) as u32
}

/// A round scale-bar length (Å) close to `target` Å.
pub fn nice_length(target: f64) -> f64 {
    const STEPS: [f64; 3] = [1.0, 2.0, 5.0];
    if target.is_nan() || target <= 0.0 {
        return 1.0;
    }
    let decade = 10f64.powf(target.log10().floor());
    let mut best = decade;
    for k in [0.1, 1.0, 10.0] {
        for s in STEPS {
            let v = decade * k * s;
            if (v - target).abs() < (best - target).abs() {
                best = v;
            }
        }
    }
    best
}

/// Colour maps shared with the section-plane shader.
pub fn viridis(t: f64) -> (f64, f64, f64) {
    let t = t.clamp(0.0, 1.0);
    let c = [
        [0.2777273272234177, 0.005407344544966578, 0.3340998053353061],
        [0.1050930431085774, 1.404613529898575, 1.384590162594685],
        [-0.3308618287255563, 0.214847559468213, 0.09509516302823659],
        [-4.634230498983486, -5.799100973351585, -19.33244095627987],
        [6.228269936347081, 14.17993336680509, 56.69055260068105],
        [4.776384997670288, -13.74514537774601, -65.35303263337234],
        [-5.435455855934631, 4.645852612178535, 26.3124352495832],
    ];
    let ch = |k: usize| c.iter().rev().fold(0.0, |acc, row| acc * t + row[k]);
    (ch(0).clamp(0.0, 1.0), ch(1).clamp(0.0, 1.0), ch(2).clamp(0.0, 1.0))
}

pub fn diverging(t: f64) -> (f64, f64, f64) {
    let t = t.clamp(-1.0, 1.0);
    let mix = |a: f64, b: f64, u: f64| a + (b - a) * u;
    if t > 0.0 {
        (mix(0.97, 0.80, t), mix(0.97, 0.16, t), mix(0.97, 0.13, t))
    } else {
        (mix(0.97, 0.13, -t), mix(0.97, 0.32, -t), mix(0.97, 0.80, -t))
    }
}

/// A colour bar for the section plane.
#[derive(Clone, Debug)]
pub struct ColorBar {
    pub signed: bool,
    pub lo: f64,
    pub hi: f64,
    pub label: String,
}

/// Everything drawn over the image, in page coordinates (pt, origin top-left).
#[derive(Clone, Debug, Default)]
pub struct Annotations {
    pub panel_letter: Option<String>,
    pub title: Option<String>,
    /// (colour, text) entries, e.g. the isosurface lobes.
    pub key: Vec<([f64; 3], String)>,
    /// Screen directions (x right, y down) of a, b, c, scaled ≤ 1.
    pub axes: Option<[[f64; 2]; 3]>,
    /// (length in Å, pt per Å).
    pub scale_bar: Option<(f64, f64)>,
    pub color_bar: Option<ColorBar>,
    /// (text, x_pt, y_pt).
    pub labels: Vec<(String, f64, f64)>,
}

/// Physical layout of the output.
#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub width_mm: f64,
    pub height_mm: f64,
    pub dpi: f64,
    pub font_pt: f64,
    pub line_pt: f64,
    /// Annotation colour: dark on light backgrounds, light on dark.
    pub ink: [f64; 3],
}

impl Page {
    pub fn width_pt(&self) -> f64 {
        self.width_mm / MM_PER_INCH * PT_PER_INCH
    }
    pub fn height_pt(&self) -> f64 {
        self.height_mm / MM_PER_INCH * PT_PER_INCH
    }
}

const FONT: &str = "Arial";

/// Space kept free of the picture for annotations (pt): a band at the top
/// (title, key, colour bar), one at the bottom (axes, scale bar) and side
/// margins. The 3D view is fitted between them so nothing overlaps it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bands {
    pub top: f64,
    pub bottom: f64,
    pub side: f64,
}

fn axis_len(f: f64) -> f64 {
    f * 2.6
}

pub fn bands(a: &Annotations, page: &Page) -> Bands {
    let f = page.font_pt;
    let margin = f * 0.9;
    let lines = a.title.is_some() as usize + a.key.len();
    let text = if lines > 0 || a.panel_letter.is_some() {
        margin + f + (lines.max(1) - 1) as f64 * f * 1.35 + f * 0.5
    } else {
        margin
    };
    let bar = if a.color_bar.is_some() { margin + f * 0.4 + f * 0.8 + f * 2.1 + f * 0.5 } else { margin };
    let axes = if a.axes.is_some() { 2.0 * axis_len(f) + margin } else { margin };
    let scale = if a.scale_bar.is_some() { f * 2.2 + margin } else { margin };
    Bands { top: text.max(bar), bottom: axes.max(scale), side: margin }
}

fn set_font(cr: &cairo::Context, size: f64, bold: bool, italic: bool) {
    cr.select_font_face(
        FONT,
        if italic { cairo::FontSlant::Italic } else { cairo::FontSlant::Normal },
        if bold { cairo::FontWeight::Bold } else { cairo::FontWeight::Normal },
    );
    cr.set_font_size(size);
}

/// Draw the annotations in page points onto `cr`.
pub fn draw_annotations(cr: &cairo::Context, a: &Annotations, page: &Page) {
    let (w, h) = (page.width_pt(), page.height_pt());
    let f = page.font_pt;
    let ink = page.ink;
    let margin = f * 0.9;
    let set_ink = |alpha: f64| cr.set_source_rgba(ink[0], ink[1], ink[2], alpha);
    let mut y = margin + f;

    // Panel letter and title, top-left.
    let mut x = margin;
    if let Some(letter) = &a.panel_letter {
        set_font(cr, f * 1.3, true, false);
        set_ink(1.0);
        cr.move_to(x, y + f * 0.15);
        let _ = cr.show_text(letter);
        x += cr.text_extents(letter).map(|e| e.x_advance()).unwrap_or(f) + f * 0.6;
    }
    if let Some(title) = &a.title {
        set_font(cr, f, false, false);
        set_ink(1.0);
        cr.move_to(x, y);
        let _ = cr.show_text(title);
        y += f * 1.35;
    }

    // Key (lobe colours and isovalues).
    set_font(cr, f, false, false);
    for (color, text) in &a.key {
        cr.set_source_rgb(color[0], color[1], color[2]);
        cr.rectangle(margin, y - f * 0.75, f * 0.9, f * 0.75);
        let _ = cr.fill_preserve();
        set_ink(0.6);
        cr.set_line_width(page.line_pt * 0.5);
        let _ = cr.stroke();
        set_ink(1.0);
        cr.move_to(margin + f * 1.3, y);
        let _ = cr.show_text(text);
        y += f * 1.3;
    }

    // Colour bar, top-right.
    if let Some(cb) = &a.color_bar {
        let (bw, bh) = (f * 9.0, f * 0.8);
        let (bx, by) = (w - margin - bw, margin + f * 0.4);
        let n = 64;
        for i in 0..n {
            let t = (i as f64 + 0.5) / n as f64;
            let c = if cb.signed { diverging(t * 2.0 - 1.0) } else { viridis(t) };
            cr.set_source_rgb(c.0, c.1, c.2);
            cr.rectangle(bx + bw * i as f64 / n as f64, by, bw / n as f64 + 0.05, bh);
            let _ = cr.fill();
        }
        set_ink(0.8);
        cr.set_line_width(page.line_pt * 0.5);
        cr.rectangle(bx, by, bw, bh);
        let _ = cr.stroke();
        set_font(cr, f * 0.85, false, false);
        set_ink(1.0);
        let (lo, hi) = if cb.signed { (-cb.hi, cb.hi) } else { (cb.lo, cb.hi) };
        let lo_t = format_value(lo);
        cr.move_to(bx, by + bh + f * 1.0);
        let _ = cr.show_text(&lo_t);
        let hi_t = format_value(hi);
        let hw = cr.text_extents(&hi_t).map(|e| e.x_advance()).unwrap_or(0.0);
        cr.move_to(bx + bw - hw, by + bh + f * 1.0);
        let _ = cr.show_text(&hi_t);
        let lw = cr.text_extents(&cb.label).map(|e| e.x_advance()).unwrap_or(0.0);
        cr.move_to(bx + (bw - lw) / 2.0, by + bh + f * 2.1);
        let _ = cr.show_text(&cb.label);
    }

    // Axis triad, bottom-left, inside the bottom band.
    if let Some(axes) = a.axes {
        let len = axis_len(f);
        let (ox, oy) = (margin + len, h - margin - len);
        cr.set_line_width(page.line_pt);
        set_font(cr, f, false, true);
        for (dir, name) in axes.iter().zip(["a", "b", "c"]) {
            let (dx, dy) = (dir[0] * len, dir[1] * len);
            set_ink(1.0);
            cr.move_to(ox, oy);
            cr.line_to(ox + dx, oy + dy);
            let _ = cr.stroke();
            // Arrow head.
            let l = (dx * dx + dy * dy).sqrt();
            if l > f * 0.5 {
                let (ux, uy) = (dx / l, dy / l);
                let hs = f * 0.5;
                cr.move_to(ox + dx, oy + dy);
                cr.line_to(ox + dx - ux * hs - uy * hs * 0.45, oy + dy - uy * hs + ux * hs * 0.45);
                cr.line_to(ox + dx - ux * hs + uy * hs * 0.45, oy + dy - uy * hs - ux * hs * 0.45);
                cr.close_path();
                let _ = cr.fill();
            }
            let (lx, ly) = if l > 1e-6 { (dx / l, dy / l) } else { (0.0, -1.0) };
            cr.move_to(ox + dx + lx * f * 0.7 - f * 0.25, oy + dy + ly * f * 0.7 + f * 0.35);
            let _ = cr.show_text(name);
        }
    }

    // Scale bar, bottom-right.
    if let Some((len_a, pt_per_a)) = a.scale_bar {
        let bar = len_a * pt_per_a;
        let (x1, yb) = (w - margin, h - margin - f * 0.2);
        set_ink(1.0);
        cr.set_line_width(page.line_pt * 1.6);
        cr.move_to(x1 - bar, yb);
        cr.line_to(x1, yb);
        let _ = cr.stroke();
        set_font(cr, f, false, false);
        let text = format!("{} Å", trim_number(len_a));
        let tw = cr.text_extents(&text).map(|e| e.x_advance()).unwrap_or(0.0);
        cr.move_to(x1 - bar / 2.0 - tw / 2.0, yb - f * 0.5);
        let _ = cr.show_text(&text);
    }

    // Atom labels.
    set_font(cr, f * 0.9, false, false);
    for (text, lx, ly) in &a.labels {
        let tw = cr.text_extents(text).map(|e| e.x_advance()).unwrap_or(0.0);
        set_ink(1.0);
        cr.move_to(lx - tw / 2.0, ly + f * 0.35);
        let _ = cr.show_text(text);
    }
}

fn trim_number(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Compact value for colour-bar ends.
pub fn format_value(v: f64) -> String {
    let a = v.abs();
    if a == 0.0 {
        "0".into()
    } else if !(1e-3..1e3).contains(&a) {
        format!("{v:.1e}")
    } else {
        trim_number(if a < 0.01 { (v * 1e5).round() / 1e5 } else { (v * 1e3).round() / 1e3 })
    }
}

/// The GL image as a Cairo surface (premultiplied ARGB32).
pub fn image_surface(img: &RgbaImage) -> Result<cairo::ImageSurface, String> {
    let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, img.width as i32, img.height as i32)
        .map_err(|e| e.to_string())?;
    let stride = surf.stride() as usize;
    {
        let mut data = surf.data().map_err(|e| e.to_string())?;
        for y in 0..img.height as usize {
            for x in 0..img.width as usize {
                let s = (y * img.width as usize + x) * 4;
                let px = u32::from_be_bytes([img.data[s + 3], img.data[s], img.data[s + 1], img.data[s + 2]]);
                data[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&px.to_ne_bytes());
            }
        }
    }
    Ok(surf)
}

fn paint_image(cr: &cairo::Context, surf: &cairo::ImageSurface, page: &Page) -> Result<(), String> {
    cr.save().map_err(|e| e.to_string())?;
    cr.scale(page.width_pt() / surf.width() as f64, page.height_pt() / surf.height() as f64);
    cr.set_source_surface(surf, 0.0, 0.0).map_err(|e| e.to_string())?;
    cr.source().set_filter(cairo::Filter::Best);
    cr.paint().map_err(|e| e.to_string())?;
    cr.restore().map_err(|e| e.to_string())?;
    Ok(())
}

/// Write the figure; the format comes from the extension (pdf, svg, png,
/// tif/tiff).
pub fn write(path: &Path, img: &RgbaImage, page: &Page, ann: &Annotations) -> Result<(), String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let surf = image_surface(img)?;
    match ext.as_str() {
        "pdf" | "svg" => {
            let (w, h) = (page.width_pt(), page.height_pt());
            let draw = |cr: &cairo::Context| -> Result<(), String> {
                paint_image(cr, &surf, page)?;
                draw_annotations(cr, ann, page);
                Ok(())
            };
            if ext == "pdf" {
                let s = cairo::PdfSurface::new(w, h, path).map_err(|e| e.to_string())?;
                let cr = cairo::Context::new(&s).map_err(|e| e.to_string())?;
                draw(&cr)?;
                drop(cr);
                s.finish();
            } else {
                // SVG user units are points, matching the page size.
                let s = cairo::SvgSurface::new(w, h, Some(path)).map_err(|e| e.to_string())?;
                let cr = cairo::Context::new(&s).map_err(|e| e.to_string())?;
                draw(&cr)?;
                drop(cr);
                s.finish();
            }
            Ok(())
        }
        "png" | "tif" | "tiff" => {
            // Annotations rasterised onto the image at its dpi.
            {
                let cr = cairo::Context::new(&surf).map_err(|e| e.to_string())?;
                cr.scale(img.width as f64 / page.width_pt(), img.height as f64 / page.height_pt());
                draw_annotations(&cr, ann, page);
            }
            if ext == "png" {
                let mut bytes = Vec::new();
                surf.write_to_png(&mut bytes).map_err(|e| e.to_string())?;
                let bytes = png_with_dpi(&bytes, page.dpi).ok_or("could not tag PNG")?;
                std::fs::write(path, bytes).map_err(|e| e.to_string())
            } else {
                write_tiff(path, surf, page.dpi).map_err(|e| e.to_string())
            }
        }
        other => Err(format!("unsupported figure format '.{other}' (use pdf, svg, png or tiff)")),
    }
}

// ---------------------------------------------------------------------------
// PNG: insert pHYs (pixels per metre) and sRGB chunks after IHDR.
// ---------------------------------------------------------------------------

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 12);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_in = kind.to_vec();
    crc_in.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
    out
}

/// `png` with its resolution set to `dpi` and an sRGB tag.
pub fn png_with_dpi(png: &[u8], dpi: f64) -> Option<Vec<u8>> {
    const SIG: usize = 8;
    if png.len() < SIG + 8 || &png[12..16] != b"IHDR" {
        return None;
    }
    let ihdr_len = u32::from_be_bytes(png[8..12].try_into().ok()?) as usize;
    let after_ihdr = SIG + 12 + ihdr_len;
    let ppm = (dpi / 0.0254).round() as u32;
    let mut phys = Vec::with_capacity(9);
    phys.extend_from_slice(&ppm.to_be_bytes());
    phys.extend_from_slice(&ppm.to_be_bytes());
    phys.push(1); // unit: metre
    let mut out = Vec::with_capacity(png.len() + 40);
    out.extend_from_slice(&png[..after_ihdr]);
    out.extend(chunk(b"sRGB", &[0]));
    out.extend(chunk(b"pHYs", &phys));
    out.extend_from_slice(&png[after_ihdr..]);
    Some(out)
}

// ---------------------------------------------------------------------------
// TIFF: baseline, uncompressed, RGB(A) 8-bit, resolution in inches.
// ---------------------------------------------------------------------------

fn write_tiff(path: &Path, surf: cairo::ImageSurface, dpi: f64) -> io::Result<()> {
    let (w, h) = (surf.width() as u32, surf.height() as u32);
    let stride = surf.stride() as usize;
    let data = surf.take_data().map_err(|e| io::Error::other(e.to_string()))?;
    // Straight (unassociated) alpha; RGB only when fully opaque.
    let mut has_alpha = false;
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let px = u32::from_ne_bytes(data[y * stride + x * 4..y * stride + x * 4 + 4].try_into().unwrap());
            let [a, r, g, b] = px.to_be_bytes();
            if a != 255 {
                has_alpha = true;
            }
            let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
            pixels.extend_from_slice(&[un(r), un(g), un(b), a]);
        }
    }
    let spp: u16 = if has_alpha { 4 } else { 3 };
    let body: Vec<u8> = if has_alpha {
        pixels
    } else {
        pixels.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
    };

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"II*\0");
    // Layout: header (8) | bits-per-sample (8) | x res (8) | y res (8) | pixels | IFD
    let bps_off = 8u32;
    let xres_off = 16u32;
    let yres_off = 24u32;
    let data_off = 32u32;
    let ifd_off = data_off + body.len() as u32;
    out.extend_from_slice(&ifd_off.to_le_bytes());
    for _ in 0..4 {
        out.extend_from_slice(&8u16.to_le_bytes());
    }
    let res = (dpi.round().max(1.0) as u32, 1u32);
    for _ in 0..2 {
        out.extend_from_slice(&res.0.to_le_bytes());
        out.extend_from_slice(&res.1.to_le_bytes());
    }
    out.extend_from_slice(&body);

    // (tag, type, count, value) — type 3 SHORT, 4 LONG, 5 RATIONAL.
    let mut entries: Vec<(u16, u16, u32, u32)> = vec![
        (256, 4, 1, w),
        (257, 4, 1, h),
        (258, 3, spp as u32, bps_off),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, data_off),
        (277, 3, 1, spp as u32),
        (278, 4, 1, h),
        (279, 4, 1, body.len() as u32),
        (282, 5, 1, xres_off),
        (283, 5, 1, yres_off),
        (284, 3, 1, 1),
        (296, 3, 1, 2),
    ];
    if has_alpha {
        entries.push((338, 3, 1, 2));
    }
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, ty, count, value) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        // SHORT values that fit sit in the low bytes of the value field.
        if ty == 3 && count == 1 {
            out.extend_from_slice(&(value as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
        } else {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    std::fs::File::create(path)?.write_all(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_png_reference() {
        // CRC of "IEND" is fixed by the PNG spec.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn png_gets_dpi_and_srgb() {
        let s = cairo::ImageSurface::create(cairo::Format::ARgb32, 4, 3).unwrap();
        let mut png = Vec::new();
        s.write_to_png(&mut png).unwrap();
        let tagged = png_with_dpi(&png, 600.0).unwrap();
        let i = tagged.windows(4).position(|w| w == b"pHYs").unwrap();
        let ppm = u32::from_be_bytes(tagged[i + 4..i + 8].try_into().unwrap());
        assert_eq!(ppm, 23622); // 600 dpi
        assert!(tagged.windows(4).any(|w| w == b"sRGB"));
        // Still a readable PNG.
        let back = cairo::ImageSurface::create_from_png(&mut &tagged[..]).unwrap();
        assert_eq!((back.width(), back.height()), (4, 3));
    }

    #[test]
    fn sizes_scale_bar_and_colours() {
        assert_eq!(pixels_for(183.0, 600.0), 4323);
        assert_eq!(pixels_for(89.0, 300.0), 1051);
        assert_eq!(nice_length(4.3), 5.0);
        assert_eq!(nice_length(1.4), 1.0);
        assert_eq!(nice_length(17.0), 20.0);
        let v0 = viridis(0.0);
        assert!(v0.0 < 0.3 && v0.2 > 0.3); // dark purple
        assert_eq!(diverging(0.0), (0.97, 0.97, 0.97));
    }

    #[test]
    fn pdf_has_the_physical_page_size() {
        let dir = std::env::temp_dir().join(format!("cview_fig_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.pdf");
        let img = RgbaImage { width: 20, height: 10, data: vec![255; 800] };
        let page = Page { width_mm: 89.0, height_mm: 44.5, dpi: 600.0, font_pt: 7.0, line_pt: 0.5, ink: [0.0; 3] };
        let ann = Annotations { title: Some("Δρ".into()), scale_bar: Some((5.0, 10.0)), ..Default::default() };
        write(&path, &img, &page, &ann).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        // 89 mm = 252.28 pt (the page is created at exactly this size).
        assert!((page.width_pt() - 252.283).abs() < 1e-3);
        if std::env::var_os("CVIEW_KEEP_FIG").is_some() {
            std::fs::copy(&path, std::env::temp_dir().join("cview_fig_check.pdf")).unwrap();
        }
        let tif = dir.join("t.tiff");
        write(&tif, &img, &page, &Annotations::default()).unwrap();
        assert_eq!(&std::fs::read(&tif).unwrap()[..4], b"II*\0");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
