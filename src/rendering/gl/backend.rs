// src/rendering/gl/backend.rs
//
// GPU drawing behind a small interface (`GpuBackend`). The GL implementation
// draws, per frame:
//   - atoms as instanced sphere impostors: one screen-aligned quad per atom,
//     ray-cast in the fragment shader with exact depth, so intersections are
//     correct and a million atoms cost a million quads;
//   - bonds as instanced cylinder impostors;
//   - line segments (the cell).
//
// GLSL 330 core only: no geometry or compute shaders, no extensions, so the
// same code runs on Mesa, Windows drivers and macOS's GL 4.1 core.

use super::camera::Camera;
use glow::HasContext;

/// One sphere. `#[repr(C)]` so a slice uploads as-is.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AtomInstance {
    pub pos: [f32; 3],
    pub radius: f32,
    pub color: [f32; 3],
}

/// One bond, coloured by halves.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BondInstance {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub radius: f32,
    pub color_a: [f32; 3],
    pub color_b: [f32; 3],
}

/// What the GPU windows need from a renderer.
pub trait GpuBackend {
    fn set_atoms(&mut self, atoms: &[AtomInstance]);
    fn set_bonds(&mut self, bonds: &[BondInstance]);
    /// Pairs of endpoints, drawn as 1-pixel lines.
    fn set_lines(&mut self, segments: &[[f32; 3]], color: [f32; 3]);
    fn draw(&mut self, camera: &Camera, width: i32, height: i32, background: [f32; 3]);
    /// RGBA8 rows, bottom row first, of the framebuffer just drawn.
    fn read_pixels(&self, width: i32, height: i32) -> Vec<u8>;
    /// Free GPU objects. The context must be current.
    fn destroy(&mut self);
}

const SPHERE_VS: &str = r#"#version 330 core
layout(location = 0) in vec2 corner;
layout(location = 1) in vec3 center;
layout(location = 2) in float radius;
layout(location = 3) in vec3 color;
uniform mat4 view;
uniform mat4 proj;
out vec2 v_uv;
out vec3 v_center;
out float v_radius;
out vec3 v_color;
void main() {
    vec4 c = view * vec4(center, 1.0);
    v_uv = corner;
    v_center = c.xyz;
    v_radius = radius;
    v_color = color;
    gl_Position = proj * vec4(c.xyz + vec3(corner * radius, 0.0), 1.0);
}
"#;

/// Shared lighting: a key light from the upper left, soft fill, a highlight,
/// and a darkened rim so neighbouring spheres separate.
const SHADE: &str = r#"
vec3 shade(vec3 base, vec3 n) {
    vec3 L = normalize(vec3(-0.45, 0.55, 1.0));
    vec3 H = normalize(L + vec3(0.0, 0.0, 1.0));
    float diff = max(dot(n, L), 0.0);
    float spec = pow(max(dot(n, H), 0.0), 60.0);
    float rim = mix(0.55, 1.0, smoothstep(0.0, 0.45, n.z));
    return (base * (0.30 + 0.70 * diff) + vec3(0.45) * spec) * rim;
}
"#;

const SPHERE_FS: &str = r#"
in vec2 v_uv;
in vec3 v_center;
in float v_radius;
in vec3 v_color;
uniform mat4 proj;
out vec4 frag;
void main() {
    float r2 = dot(v_uv, v_uv);
    if (r2 > 1.0) discard;
    vec3 n = vec3(v_uv, sqrt(1.0 - r2));
    vec4 clip = proj * vec4(v_center + n * v_radius, 1.0);
    gl_FragDepth = clip.z / clip.w * 0.5 + 0.5;
    frag = vec4(shade(v_color, n), 1.0);
}
"#;

const BOND_VS: &str = r#"#version 330 core
layout(location = 0) in vec2 corner;
layout(location = 1) in vec3 a;
layout(location = 2) in vec3 b;
layout(location = 3) in float radius;
layout(location = 4) in vec3 color_a;
layout(location = 5) in vec3 color_b;
uniform mat4 view;
uniform mat4 proj;
out float v_s;
out float v_t;
out float v_z;
out float v_radius;
flat out vec2 v_perp;
out vec3 v_ca;
out vec3 v_cb;
void main() {
    vec3 av = (view * vec4(a, 1.0)).xyz;
    vec3 bv = (view * vec4(b, 1.0)).xyz;
    vec2 d = bv.xy - av.xy;
    float len = length(d);
    vec2 dir = len > 1e-6 ? d / len : vec2(1.0, 0.0);
    vec2 perp = vec2(-dir.y, dir.x);
    float t = corner.x * 0.5 + 0.5;
    vec3 p = mix(av, bv, t) + vec3(perp * corner.y * radius, 0.0);
    v_s = corner.y;
    v_t = t;
    v_z = mix(av.z, bv.z, t);
    v_radius = radius;
    v_perp = perp;
    v_ca = color_a;
    v_cb = color_b;
    gl_Position = proj * vec4(p, 1.0);
}
"#;

const BOND_FS: &str = r#"
in float v_s;
in float v_t;
in float v_z;
in float v_radius;
flat in vec2 v_perp;
in vec3 v_ca;
in vec3 v_cb;
uniform mat4 proj;
out vec4 frag;
void main() {
    float h = sqrt(max(1.0 - v_s * v_s, 0.0));
    vec4 clip = proj * vec4(0.0, 0.0, v_z + h * v_radius, 1.0);
    gl_FragDepth = clip.z / clip.w * 0.5 + 0.5;
    vec3 n = vec3(v_perp * v_s, h);
    frag = vec4(shade(v_t < 0.5 ? v_ca : v_cb, n), 1.0);
}
"#;

const LINE_VS: &str = r#"#version 330 core
layout(location = 0) in vec3 pos;
uniform mat4 view;
uniform mat4 proj;
void main() { gl_Position = proj * view * vec4(pos, 1.0); }
"#;

const LINE_FS: &str = r#"#version 330 core
uniform vec3 color;
out vec4 frag;
void main() { frag = vec4(color, 1.0); }
"#;

struct Program {
    program: glow::Program,
    view: Option<glow::UniformLocation>,
    proj: Option<glow::UniformLocation>,
    color: Option<glow::UniformLocation>,
}

pub struct GlBackend {
    gl: glow::Context,
    sphere: Program,
    bond: Program,
    line: Program,
    quad: glow::Buffer,
    sphere_vao: glow::VertexArray,
    sphere_vbo: glow::Buffer,
    bond_vao: glow::VertexArray,
    bond_vbo: glow::Buffer,
    line_vao: glow::VertexArray,
    line_vbo: glow::Buffer,
    n_atoms: i32,
    n_bonds: i32,
    n_line_verts: i32,
    line_color: [f32; 3],
}

fn as_bytes<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: T is a `#[repr(C)]` struct of f32 (or an f32 array) with no
    // padding; viewing it as bytes for upload is sound.
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

unsafe fn compile(gl: &glow::Context, vs: &str, fs: &str) -> Result<Program, String> {
    let program = gl.create_program()?;
    let mut shaders = Vec::new();
    for (kind, src) in [(glow::VERTEX_SHADER, vs.to_string()), (glow::FRAGMENT_SHADER, fs.to_string())] {
        let sh = gl.create_shader(kind)?;
        gl.shader_source(sh, &src);
        gl.compile_shader(sh);
        if !gl.get_shader_compile_status(sh) {
            return Err(format!("shader compile failed: {}", gl.get_shader_info_log(sh)));
        }
        gl.attach_shader(program, sh);
        shaders.push(sh);
    }
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        return Err(format!("shader link failed: {}", gl.get_program_info_log(program)));
    }
    for sh in shaders {
        gl.detach_shader(program, sh);
        gl.delete_shader(sh);
    }
    Ok(Program {
        view: gl.get_uniform_location(program, "view"),
        proj: gl.get_uniform_location(program, "proj"),
        color: gl.get_uniform_location(program, "color"),
        program,
    })
}

/// Bind `vbo` as per-instance attributes laid out as `sizes` (in floats).
unsafe fn instance_attribs(gl: &glow::Context, quad: glow::Buffer, vbo: glow::Buffer, sizes: &[i32]) {
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad));
    gl.enable_vertex_attrib_array(0);
    gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
    let stride: i32 = sizes.iter().sum::<i32>() * 4;
    let mut offset = 0;
    for (k, &n) in sizes.iter().enumerate() {
        let loc = k as u32 + 1;
        gl.enable_vertex_attrib_array(loc);
        gl.vertex_attrib_pointer_f32(loc, n, glow::FLOAT, false, stride, offset);
        gl.vertex_attrib_divisor(loc, 1);
        offset += n * 4;
    }
}

impl GlBackend {
    /// Build programs and buffers. The GL context must be current.
    pub fn new(gl: glow::Context) -> Result<Self, String> {
        unsafe {
            let sphere = compile(&gl, SPHERE_VS, &format!("#version 330 core\n{SHADE}{SPHERE_FS}"))?;
            let bond = compile(&gl, BOND_VS, &format!("#version 330 core\n{SHADE}{BOND_FS}"))?;
            let line = compile(&gl, LINE_VS, LINE_FS)?;

            let quad = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad));
            let corners: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]];
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(&corners), glow::STATIC_DRAW);

            let sphere_vao = gl.create_vertex_array()?;
            let sphere_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(sphere_vao));
            instance_attribs(&gl, quad, sphere_vbo, &[3, 1, 3]);

            let bond_vao = gl.create_vertex_array()?;
            let bond_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(bond_vao));
            instance_attribs(&gl, quad, bond_vbo, &[3, 3, 1, 3, 3]);

            let line_vao = gl.create_vertex_array()?;
            let line_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(line_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(line_vbo));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);

            gl.bind_vertex_array(None);
            Ok(Self {
                gl,
                sphere,
                bond,
                line,
                quad,
                sphere_vao,
                sphere_vbo,
                bond_vao,
                bond_vbo,
                line_vao,
                line_vbo,
                n_atoms: 0,
                n_bonds: 0,
                n_line_verts: 0,
                line_color: [0.3, 0.3, 0.3],
            })
        }
    }

    unsafe fn set_camera(&self, p: &Program, view: &[f32], proj: &[f32]) {
        self.gl.use_program(Some(p.program));
        self.gl.uniform_matrix_4_f32_slice(p.view.as_ref(), false, view);
        self.gl.uniform_matrix_4_f32_slice(p.proj.as_ref(), false, proj);
    }
}

impl GpuBackend for GlBackend {
    fn set_atoms(&mut self, atoms: &[AtomInstance]) {
        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.sphere_vbo));
            self.gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(atoms), glow::STREAM_DRAW);
        }
        self.n_atoms = atoms.len() as i32;
    }

    fn set_bonds(&mut self, bonds: &[BondInstance]) {
        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.bond_vbo));
            self.gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(bonds), glow::STREAM_DRAW);
        }
        self.n_bonds = bonds.len() as i32;
    }

    fn set_lines(&mut self, segments: &[[f32; 3]], color: [f32; 3]) {
        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.line_vbo));
            self.gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(segments), glow::STREAM_DRAW);
        }
        self.n_line_verts = segments.len() as i32;
        self.line_color = color;
    }

    fn draw(&mut self, camera: &Camera, width: i32, height: i32, bg: [f32; 3]) {
        let view = camera.view();
        let proj = camera.projection(width as f64, height as f64);
        let gl = &self.gl;
        unsafe {
            gl.viewport(0, 0, width, height);
            gl.clear_color(bg[0], bg[1], bg[2], 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LESS);

            if self.n_atoms > 0 {
                self.set_camera(&self.sphere, view.as_slice(), proj.as_slice());
                gl.bind_vertex_array(Some(self.sphere_vao));
                gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, self.n_atoms);
            }
            if self.n_bonds > 0 {
                self.set_camera(&self.bond, view.as_slice(), proj.as_slice());
                gl.bind_vertex_array(Some(self.bond_vao));
                gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, self.n_bonds);
            }
            if self.n_line_verts > 0 {
                self.set_camera(&self.line, view.as_slice(), proj.as_slice());
                let c = self.line_color;
                gl.uniform_3_f32(self.line.color.as_ref(), c[0], c[1], c[2]);
                gl.bind_vertex_array(Some(self.line_vao));
                gl.draw_arrays(glow::LINES, 0, self.n_line_verts);
            }
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }

    fn read_pixels(&self, width: i32, height: i32) -> Vec<u8> {
        let mut buf = vec![0u8; (width.max(0) * height.max(0) * 4) as usize];
        unsafe {
            self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            self.gl.read_pixels(
                0,
                0,
                width,
                height,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut buf)),
            );
        }
        buf
    }

    fn destroy(&mut self) {
        let gl = &self.gl;
        unsafe {
            for p in [&self.sphere, &self.bond, &self.line] {
                gl.delete_program(p.program);
            }
            for b in [self.quad, self.sphere_vbo, self.bond_vbo, self.line_vbo] {
                gl.delete_buffer(b);
            }
            for v in [self.sphere_vao, self.bond_vao, self.line_vao] {
                gl.delete_vertex_array(v);
            }
        }
    }
}
