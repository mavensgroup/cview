// src/rendering/gl/export.rs
//
// Publication rendering: an image of any pixel size, independent of the
// window, at print quality.
//
// - Tiled: the output is cut into tiles that fit the GPU's framebuffer
//   limit, each rendered with its own sub-window of the orthographic
//   projection, so 183 mm at 1200 dpi works on any GPU.
// - Supersampled: each tile renders at `supersample`× and is box-filtered
//   down in linear light, then dithered to 8 bits (no banding in gradients).
// - Exact transparency: transparent meshes are depth-peeled (front to back,
//   one layer per pass until nothing is left or `max_layers`), instead of
//   the interactive view's weighted-blended approximation.
// - Optional transparent background, as premultiplied alpha.
//
// Line widths are given in output pixels and scaled by the supersampling,
// so a 0.5 pt cell edge is 0.5 pt in the file.

use super::backend::{new_texture, Frame, GlBackend};
use super::camera::Camera;
use glow::HasContext;

/// What to render.
#[derive(Clone, Debug)]
pub struct ExportRequest {
    /// View-space window (left, right, bottom, top), Å, around the camera.
    pub window: [f64; 4],
    pub width: u32,
    pub height: u32,
    pub supersample: u32,
    /// `None` for a transparent background.
    pub background: Option<[f32; 3]>,
    /// Width of lines (cell edges), in output pixels.
    pub line_width_px: f32,
    pub max_layers: u32,
}

/// Premultiplied RGBA8, top row first.
#[derive(Clone, Debug)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Deterministic per-pixel noise in [-0.5, 0.5) (triangular-ish dither).
fn dither(x: u32, y: u32, c: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    let a = (h & 0xFFFF) as f32 / 65536.0;
    let b = (h >> 16) as f32 / 65536.0;
    (a + b) * 0.5 - 0.5
}

/// Average `ss`×`ss` blocks of a premultiplied tile (bottom row first, as
/// GL reads it) in linear light into `out` at (`ox`, `oy`), top row first.
#[allow(clippy::too_many_arguments)]
fn downsample_into(
    tile: &[u8],
    tile_w: usize,
    tile_h: usize,
    ss: usize,
    out: &mut RgbaImage,
    ox: usize,
    oy: usize,
    lut: &[f32; 256],
) {
    let (tw, th) = (tile_w / ss, tile_h / ss);
    let inv = 1.0 / (ss * ss) as f32;
    for y in 0..th {
        for x in 0..tw {
            let mut acc = [0f32; 4];
            for sy in 0..ss {
                // Tile row counted from the top.
                let row_top = y * ss + sy;
                let row = tile_h - 1 - row_top;
                for sx in 0..ss {
                    let i = (row * tile_w + x * ss + sx) * 4;
                    for (c, a) in acc.iter_mut().take(3).enumerate() {
                        *a += lut[tile[i + c] as usize];
                    }
                    acc[3] += tile[i + 3] as f32 / 255.0;
                }
            }
            let (gx, gy) = (ox + x, oy + y);
            let o = (gy * out.width as usize + gx) * 4;
            for (c, a) in acc.iter().take(3).enumerate() {
                let v = linear_to_srgb(a * inv) * 255.0 + dither(gx as u32, gy as u32, c as u32);
                out.data[o + c] = v.round().clamp(0.0, 255.0) as u8;
            }
            out.data[o + 3] = (acc[3] * inv * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

struct Targets {
    textures: Vec<glow::Texture>,
    fbos: Vec<glow::Framebuffer>,
}

impl GlBackend {
    /// Render `req` offscreen. The GL context must be current. Progress is
    /// reported as the fraction of tiles done.
    pub fn render_image(&mut self, camera: &Camera, req: &ExportRequest, progress: &mut dyn FnMut(f32)) -> Result<RgbaImage, String> {
        let ss = req.supersample.clamp(1, 4) as i32;
        let saved_width = self.line_width;
        let result = unsafe { self.render_tiles(camera, req, ss, progress) };
        self.line_width = saved_width;
        result
    }

    unsafe fn render_tiles(&mut self, camera: &Camera, req: &ExportRequest, ss: i32, progress: &mut dyn FnMut(f32)) -> Result<RgbaImage, String> {
        let gl = &self.gl;
        let limit = gl
            .get_parameter_i32(glow::MAX_RENDERBUFFER_SIZE)
            .min(gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE))
            .min(4096);
        let tile_out = (limit / ss).clamp(64, 1024) as u32;
        let tile_ss = tile_out as i32 * ss;

        // Targets, at the full tile size; edge tiles use part of them.
        let mut t = Targets { textures: vec![], fbos: vec![] };
        let mut tex = |internal: u32, format: u32, ty: u32| -> Result<glow::Texture, String> {
            let x = new_texture(gl, internal, format, ty, tile_ss, tile_ss)?;
            t.textures.push(x);
            Ok(x)
        };
        let op_color = tex(glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE)?;
        let op_depth = tex(glow::DEPTH_COMPONENT24, glow::DEPTH_COMPONENT, glow::UNSIGNED_INT)?;
        let layer_color = tex(glow::RGBA16F, glow::RGBA, glow::HALF_FLOAT)?;
        let mut depth_a = tex(glow::DEPTH_COMPONENT24, glow::DEPTH_COMPONENT, glow::UNSIGNED_INT)?;
        let mut depth_b = tex(glow::DEPTH_COMPONENT24, glow::DEPTH_COMPONENT, glow::UNSIGNED_INT)?;
        let accum = tex(glow::RGBA16F, glow::RGBA, glow::HALF_FLOAT)?;
        let out_color = tex(glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE)?;
        let mut fbo = |color: glow::Texture, depth: Option<glow::Texture>| -> Result<glow::Framebuffer, String> {
            let f = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(color), 0);
            if let Some(d) = depth {
                gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::TEXTURE_2D, Some(d), 0);
            }
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            t.fbos.push(f);
            if status != glow::FRAMEBUFFER_COMPLETE {
                return Err(format!("export framebuffer incomplete (0x{status:x})"));
            }
            Ok(f)
        };
        let built: Result<_, String> = (|| {
            Ok((
                fbo(op_color, Some(op_depth))?,
                fbo(layer_color, Some(depth_a))?,
                fbo(accum, None)?,
                fbo(out_color, None)?,
            ))
        })();
        let (op_fbo, layer_fbo, accum_fbo, out_fbo) = match built {
            Ok(f) => f,
            Err(e) => {
                self.free_targets(t);
                return Err(e);
            }
        };

        let mut lut = [0f32; 256];
        for (i, v) in lut.iter_mut().enumerate() {
            *v = srgb_to_linear(i as f32 / 255.0);
        }
        let mut image = RgbaImage {
            width: req.width,
            height: req.height,
            data: vec![0; req.width as usize * req.height as usize * 4],
        };
        let [l, r, b, top] = req.window;
        let (w_out, h_out) = (req.width as f64, req.height as f64);
        let tiles_x = req.width.div_ceil(tile_out);
        let tiles_y = req.height.div_ceil(tile_out);
        let any_transparent = self.any_transparent();
        let view = camera.view();
        let clip = self.clip_in_view(&view);
        self.line_width = req.line_width_px * ss as f32;
        let bg = req.background;
        let query = gl.create_query().ok();
        let mut buf = vec![0u8; (tile_ss * tile_ss * 4) as usize];

        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let (x0, y0) = (tx * tile_out, ty * tile_out);
                let x1 = (x0 + tile_out).min(req.width);
                let y1 = (y0 + tile_out).min(req.height);
                let (tw, th) = ((x1 - x0) as i32 * ss, (y1 - y0) as i32 * ss);
                let proj = camera.projection_window(
                    l + (r - l) * x0 as f64 / w_out,
                    l + (r - l) * x1 as f64 / w_out,
                    top - (top - b) * y1 as f64 / h_out,
                    top - (top - b) * y0 as f64 / h_out,
                );
                let frame = Frame {
                    view: view.as_slice(),
                    proj: proj.as_slice(),
                    clip,
                    viewport: (tw, th),
                };

                // Opaque scene, with a depth texture the peeling tests against.
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(op_fbo));
                gl.draw_buffers(&[glow::COLOR_ATTACHMENT0]);
                gl.viewport(0, 0, tw, th);
                match bg {
                    Some(c) => gl.clear_color(c[0], c[1], c[2], 1.0),
                    None => gl.clear_color(0.0, 0.0, 0.0, 0.0),
                }
                gl.depth_mask(true);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                gl.enable(glow::DEPTH_TEST);
                gl.depth_func(glow::LESS);
                gl.disable(glow::BLEND);
                self.draw_primitives(&frame);
                self.draw_meshes(&frame, false, 0);

                // Transparent layers, nearest first, accumulated "under".
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(accum_fbo));
                gl.viewport(0, 0, tw, th);
                gl.clear_color(0.0, 0.0, 0.0, 0.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
                if any_transparent {
                    // "Previous layer" starts at depth 0: nothing peeled yet.
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(layer_fbo));
                    gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::TEXTURE_2D, Some(depth_b), 0);
                    gl.clear_buffer_f32_slice(glow::DEPTH, 0, &[0.0]);
                    for _ in 0..req.max_layers.max(1) {
                        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(layer_fbo));
                        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::TEXTURE_2D, Some(depth_a), 0);
                        gl.viewport(0, 0, tw, th);
                        gl.clear_color(0.0, 0.0, 0.0, 0.0);
                        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                        gl.enable(glow::DEPTH_TEST);
                        gl.depth_mask(true);
                        gl.disable(glow::BLEND);
                        gl.active_texture(glow::TEXTURE3);
                        gl.bind_texture(glow::TEXTURE_2D, Some(depth_b));
                        gl.active_texture(glow::TEXTURE4);
                        gl.bind_texture(glow::TEXTURE_2D, Some(op_depth));
                        gl.active_texture(glow::TEXTURE0);
                        if let Some(q) = query {
                            gl.begin_query(glow::SAMPLES_PASSED, q);
                        }
                        self.draw_meshes(&frame, true, 3);
                        let passed = match query {
                            Some(q) => {
                                gl.end_query(glow::SAMPLES_PASSED);
                                gl.get_query_parameter_u32(q, glow::QUERY_RESULT)
                            }
                            None => 1,
                        };
                        // Unbind the depth textures before they become targets again.
                        gl.active_texture(glow::TEXTURE3);
                        gl.bind_texture(glow::TEXTURE_2D, None);
                        gl.active_texture(glow::TEXTURE0);
                        if passed == 0 {
                            break;
                        }
                        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(accum_fbo));
                        gl.disable(glow::DEPTH_TEST);
                        gl.enable(glow::BLEND);
                        gl.blend_func(glow::ONE_MINUS_DST_ALPHA, glow::ONE);
                        gl.use_program(Some(self.copy_prog.program));
                        gl.bind_texture(glow::TEXTURE_2D, Some(layer_color));
                        gl.bind_vertex_array(Some(self.empty_vao));
                        gl.draw_arrays(glow::TRIANGLES, 0, 3);
                        gl.disable(glow::BLEND);
                        std::mem::swap(&mut depth_a, &mut depth_b);
                    }
                }

                // Transparent over opaque, into the output tile.
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(out_fbo));
                gl.viewport(0, 0, tw, th);
                gl.disable(glow::DEPTH_TEST);
                gl.disable(glow::BLEND);
                gl.use_program(Some(self.resolve_prog.program));
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(op_color));
                gl.active_texture(glow::TEXTURE1);
                gl.bind_texture(glow::TEXTURE_2D, Some(accum));
                gl.active_texture(glow::TEXTURE0);
                gl.bind_vertex_array(Some(self.empty_vao));
                gl.draw_arrays(glow::TRIANGLES, 0, 3);

                gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
                let n = (tw * th * 4) as usize;
                gl.read_pixels(0, 0, tw, th, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut buf[..n])));
                downsample_into(&buf[..n], tw as usize, th as usize, ss as usize, &mut image, x0 as usize, y0 as usize, &lut);
                progress((ty * tiles_x + tx + 1) as f32 / (tiles_x * tiles_y) as f32);
            }
        }

        if let Some(q) = query {
            gl.delete_query(q);
        }
        gl.bind_vertex_array(None);
        gl.use_program(None);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        self.free_targets(t);
        Ok(image)
    }

    unsafe fn free_targets(&self, t: Targets) {
        for f in t.fbos {
            self.gl.delete_framebuffer(f);
        }
        for x in t.textures {
            self.gl.delete_texture(x);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_round_trip_and_downsample_of_a_flat_tile() {
        for v in [0.0f32, 0.02, 0.5, 1.0] {
            assert!((linear_to_srgb(srgb_to_linear(v)) - v).abs() < 1e-5);
        }
        let mut lut = [0f32; 256];
        for (i, v) in lut.iter_mut().enumerate() {
            *v = srgb_to_linear(i as f32 / 255.0);
        }
        // 4x4 tile of one colour → 2x2 output of the same colour.
        let tile: Vec<u8> = (0..16).flat_map(|_| [200u8, 100, 50, 255]).collect();
        let mut out = RgbaImage { width: 2, height: 2, data: vec![0; 16] };
        downsample_into(&tile, 4, 4, 2, &mut out, 0, 0, &lut);
        for px in out.data.chunks(4) {
            assert!((px[0] as i32 - 200).abs() <= 1 && (px[1] as i32 - 100).abs() <= 1 && px[3] == 255, "{px:?}");
        }
    }

    #[test]
    fn half_covered_edge_averages_in_linear_light() {
        let mut lut = [0f32; 256];
        for (i, v) in lut.iter_mut().enumerate() {
            *v = srgb_to_linear(i as f32 / 255.0);
        }
        // Black and white halves: the linear mean is 0.5, i.e. sRGB ~188,
        // not the naive 128 that makes antialiased edges look too dark.
        let mut out2 = RgbaImage { width: 1, height: 1, data: vec![0; 4] };
        let tile2 = vec![0u8, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255];
        downsample_into(&tile2, 2, 2, 2, &mut out2, 0, 0, &lut);
        assert!((out2.data[0] as i32 - 188).abs() <= 1, "{}", out2.data[0]);
    }
}
