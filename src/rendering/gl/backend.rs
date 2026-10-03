// src/rendering/gl/backend.rs
//
// GPU drawing behind a small interface (`GpuBackend`). The GL implementation
// draws:
//   - atoms as instanced sphere impostors: one screen-aligned quad per atom,
//     ray-cast in the fragment shader with exact depth;
//   - bonds as instanced cylinder impostors;
//   - line segments (the cell) as screen-space quads of a set pixel width,
//     so lines keep a physical width in high-resolution exports;
//   - indexed triangle meshes (isosurfaces), opaque or transparent, with
//     per-vertex ambient occlusion;
//   - a section plane coloured by a 3D density texture, with contours;
//   - an optional clip plane that cuts atoms, bonds and meshes away.
//
// Without meshes it draws straight into the target (the trajectory player).
// With meshes it renders offscreen with 4x MSAA and weighted blended
// order-independent transparency, then composites. `gl::export` adds a
// tiled, supersampled, depth-peeled path for publication images.
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

/// An indexed triangle mesh to upload (an isosurface).
pub struct MeshData<'a> {
    pub positions: &'a [[f32; 3]],
    pub normals: &'a [[f32; 3]],
    pub indices: &'a [u32],
    /// Ambient occlusion per vertex (1 = open), if computed.
    pub ao: Option<&'a [f32]>,
}

/// How a mesh is shaded. Opacity below 1 draws it with order-independent
/// transparency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub color: [f32; 3],
    pub opacity: f32,
}

/// A scalar field for section planes, in e/Å³ on the periodic grid.
pub struct Volume<'a> {
    pub data: &'a [f32],
    pub dims: [usize; 3],
    /// Lattice vectors as rows, Å.
    pub lattice: [[f64; 3]; 3],
}

/// A planar section through the volume: the quad `origin ± e1 ± e2`
/// (clipped to the cell), coloured by the density.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub origin: [f32; 3],
    pub e1: [f32; 3],
    pub e2: [f32; 3],
    /// Diverging blue–white–red around 0 (difference/spin densities), else a
    /// logarithmic viridis scale for a positive density.
    pub signed: bool,
    /// Log scale range (positive map) or saturation value (signed map).
    pub lo: f32,
    pub hi: f32,
    /// Contour drawn at |ρ| = iso, matching the isosurface; 0 for none.
    pub iso: f32,
}

/// What the GPU windows need from a renderer.
pub trait GpuBackend {
    fn set_atoms(&mut self, atoms: &[AtomInstance]);
    fn set_bonds(&mut self, bonds: &[BondInstance]);
    /// Pairs of endpoints, drawn as lines `set_line_width` pixels wide.
    fn set_lines(&mut self, segments: &[[f32; 3]], color: [f32; 3]);
    fn set_line_width(&mut self, px: f32);
    /// Put a mesh in `slot` (replacing what was there), or clear the slot.
    fn set_mesh(&mut self, slot: usize, mesh: Option<MeshData>);
    fn set_mesh_material(&mut self, slot: usize, material: Material);
    /// How strongly ambient occlusion darkens meshes (0 = off).
    fn set_ao_strength(&mut self, strength: f32);
    /// The field section planes sample (uploaded as a 3D texture).
    fn set_volume(&mut self, volume: Option<Volume>);
    fn set_section(&mut self, section: Option<Section>);
    /// Keep only the half-space `normal · x <= d` (world, Å), or no clipping.
    fn set_clip(&mut self, clip: Option<([f32; 3], f32)>);
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
uniform vec4 clip;
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
    // A clipped atom is removed whole: half-spheres read as artefacts.
    if (dot(clip.xyz, clip.xyz) > 0.0 && dot(clip.xyz, c.xyz) > clip.w) {
        gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
        return;
    }
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
layout(location = 0) out vec4 frag;
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
out vec3 v_pos;
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
    v_pos = p;
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
in vec3 v_pos;
flat in vec2 v_perp;
in vec3 v_ca;
in vec3 v_cb;
uniform mat4 proj;
uniform vec4 clip;
layout(location = 0) out vec4 frag;
void main() {
    if (dot(clip.xyz, clip.xyz) > 0.0 && dot(clip.xyz, v_pos) > clip.w) discard;
    float h = sqrt(max(1.0 - v_s * v_s, 0.0));
    vec4 c = proj * vec4(0.0, 0.0, v_z + h * v_radius, 1.0);
    gl_FragDepth = c.z / c.w * 0.5 + 0.5;
    vec3 n = vec3(v_perp * v_s, h);
    frag = vec4(shade(v_t < 0.5 ? v_ca : v_cb, n), 1.0);
}
"#;

/// Lines as screen-space quads: `width_px` wide whatever the resolution.
const LINE_VS: &str = r#"#version 330 core
layout(location = 0) in vec2 corner;
layout(location = 1) in vec3 a;
layout(location = 2) in vec3 b;
uniform mat4 view;
uniform mat4 proj;
uniform vec2 viewport;
uniform float width_px;
void main() {
    vec4 ca = proj * view * vec4(a, 1.0);
    vec4 cb = proj * view * vec4(b, 1.0);
    vec2 d = (cb.xy / cb.w - ca.xy / ca.w) * viewport;
    float l = length(d);
    vec2 dir = l > 1e-6 ? d / l : vec2(1.0, 0.0);
    vec2 n = vec2(-dir.y, dir.x);
    vec4 c = mix(ca, cb, corner.x * 0.5 + 0.5);
    // Half the width each side, and the ends extended so corners close.
    vec2 off = (n * corner.y + dir * corner.x) * width_px / viewport;
    c.xy += off * c.w;
    c.z -= 2e-4 * c.w;   // stay on top of a surface the line lies on
    gl_Position = c;
}
"#;

const LINE_FS: &str = r#"#version 330 core
uniform vec3 color;
layout(location = 0) out vec4 frag;
void main() { frag = vec4(color, 1.0); }
"#;

const MESH_VS: &str = r#"#version 330 core
layout(location = 0) in vec3 pos;
layout(location = 1) in vec3 nrm;
layout(location = 2) in float ao;
uniform mat4 view;
uniform mat4 proj;
out vec3 v_n;
out vec3 v_pos;
out float v_ao;
void main() {
    vec4 p = view * vec4(pos, 1.0);
    v_n = mat3(view) * nrm;
    v_pos = p.xyz;
    v_ao = ao;
    gl_Position = proj * p;
}
"#;

/// `mode` 0: opaque. 1/2: weighted blended OIT accumulation / revealage
/// (McGuire & Bavoil 2013; two passes, as GL 3.3 has no per-target blend
/// functions). 3: one depth-peeling layer, premultiplied, for exports.
const MESH_FS: &str = r#"
in vec3 v_n;
in vec3 v_pos;
in float v_ao;
uniform vec3 color;
uniform float opacity;
uniform int mode;
uniform float ao_strength;
uniform vec4 clip;
uniform sampler2D prev_depth;
uniform sampler2D opaque_depth;
layout(location = 0) out vec4 out0;
layout(location = 1) out vec4 out1;
void main() {
    if (dot(clip.xyz, clip.xyz) > 0.0 && dot(clip.xyz, v_pos) > clip.w) discard;
    vec3 n = normalize(v_n);
    if (!gl_FrontFacing) n = -n;          // inner side of an open surface
    vec3 c = shade(color, n) * mix(1.0, v_ao, ao_strength);
    // Edges seen side-on read as more opaque: lobes keep a solid outline.
    float a = opacity >= 0.999 ? 1.0
            : clamp(opacity + (1.0 - opacity) * 0.6 * pow(1.0 - abs(n.z), 3.0), 0.0, 1.0);
    out0 = vec4(0.0);
    out1 = vec4(0.0);
    if (mode == 0) {
        out0 = vec4(c, 1.0);
    } else if (mode == 3) {
        ivec2 px = ivec2(gl_FragCoord.xy);
        float z = gl_FragCoord.z;
        if (z <= texelFetch(prev_depth, px, 0).r + 1e-7 || z >= texelFetch(opaque_depth, px, 0).r) discard;
        out0 = vec4(c * a, a);
    } else {
        float w = clamp(pow(min(1.0, a * 10.0) + 0.01, 3.0) * 1e8
                        * pow(1.0 - gl_FragCoord.z * 0.9, 3.0), 1e-2, 3e3);
        if (mode == 1) out0 = vec4(c * a, a) * w;
        else out1 = vec4(a);
    }
}
"#;

const SECTION_VS: &str = r#"#version 330 core
layout(location = 0) in vec2 corner;
uniform mat4 view;
uniform mat4 proj;
uniform vec3 origin;
uniform vec3 e1;
uniform vec3 e2;
out vec3 v_world;
void main() {
    v_world = origin + corner.x * e1 + corner.y * e2;
    gl_Position = proj * view * vec4(v_world, 1.0);
}
"#;

const SECTION_FS: &str = r#"#version 330 core
in vec3 v_world;
uniform mat3 inv_lattice;
uniform sampler3D volume;
uniform vec3 dims;
uniform int signed_map;
uniform float lo;
uniform float hi;
uniform float iso;
layout(location = 0) out vec4 frag;
// Viridis, polynomial fit (Matt Zucker, CC0).
vec3 viridis(float t) {
    const vec3 c0 = vec3(0.2777273272234177, 0.005407344544966578, 0.3340998053353061);
    const vec3 c1 = vec3(0.1050930431085774, 1.404613529898575, 1.384590162594685);
    const vec3 c2 = vec3(-0.3308618287255563, 0.214847559468213, 0.09509516302823659);
    const vec3 c3 = vec3(-4.634230498983486, -5.799100973351585, -19.33244095627987);
    const vec3 c4 = vec3(6.228269936347081, 14.17993336680509, 56.69055260068105);
    const vec3 c5 = vec3(4.776384997670288, -13.74514537774601, -65.35303263337234);
    const vec3 c6 = vec3(-5.435455855934631, 4.645852612178535, 26.3124352495832);
    return c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6)))));
}
void main() {
    vec3 f = inv_lattice * v_world;
    if (any(lessThan(f, vec3(-1e-4))) || any(greaterThan(f, vec3(1.0 + 1e-4)))) discard;
    // Grid point i sits at fraction i/n; texel centres are at (i + 0.5)/n.
    float v = texture(volume, f + 0.5 / dims).r;
    vec3 c;
    if (signed_map == 1) {
        float t = clamp(v / hi, -1.0, 1.0);
        c = t > 0.0 ? mix(vec3(0.97), vec3(0.80, 0.16, 0.13), t)
                    : mix(vec3(0.97), vec3(0.13, 0.32, 0.80), -t);
    } else {
        float t = clamp(log(max(v, lo) / lo) / log(hi / lo), 0.0, 1.0);
        c = viridis(t);
    }
    if (iso > 0.0) {
        float d = abs(abs(v) - iso);
        float line = 1.0 - smoothstep(0.0, 1.5 * fwidth(v), d);
        c = mix(c, vec3(0.06), 0.85 * line);
    }
    frag = vec4(c, 1.0);
}
"#;

const FULLSCREEN_VS: &str = r#"#version 330 core
out vec2 uv;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    uv = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const COMPOSITE_FS: &str = r#"#version 330 core
in vec2 uv;
uniform sampler2D opaque_tex;
uniform sampler2D accum_tex;
uniform sampler2D reveal_tex;
uniform int has_transparent;
out vec4 frag;
void main() {
    vec3 o = texture(opaque_tex, uv).rgb;
    if (has_transparent == 0) { frag = vec4(o, 1.0); return; }
    vec4 a = texture(accum_tex, uv);
    float r = texture(reveal_tex, uv).r;
    vec3 t = a.rgb / clamp(a.a, 1e-4, 5e4);
    frag = vec4(t * (1.0 - r) + o * r, 1.0);
}
"#;

/// Copy a texture (depth-peeled layer) for blending.
pub(super) const COPY_FS: &str = r#"#version 330 core
uniform sampler2D src;
out vec4 frag;
void main() { frag = texelFetch(src, ivec2(gl_FragCoord.xy), 0); }
"#;

/// Premultiplied front-to-back transparency over the opaque scene.
pub(super) const RESOLVE_FS: &str = r#"#version 330 core
uniform sampler2D opaque_tex;
uniform sampler2D accum_tex;
out vec4 frag;
void main() {
    ivec2 px = ivec2(gl_FragCoord.xy);
    vec4 o = texelFetch(opaque_tex, px, 0);
    vec4 t = texelFetch(accum_tex, px, 0);
    frag = t + (1.0 - t.a) * o;
}
"#;

pub(super) struct GpuMesh {
    pub(super) vao: glow::VertexArray,
    pub(super) vbo: glow::Buffer,
    pub(super) ebo: glow::Buffer,
    pub(super) count: i32,
    pub(super) material: Material,
}

/// Offscreen targets for the interactive mesh path: multisampled opaque
/// colour + depth, multisampled OIT accumulation/revealage sharing that
/// depth, and single-sample textures they resolve into for compositing.
struct Pipeline {
    w: i32,
    h: i32,
    rbs: Vec<glow::Renderbuffer>,
    fbos: Vec<glow::Framebuffer>,
    texs: Vec<glow::Texture>,
    scene_fbo: glow::Framebuffer,
    oit_fbo: glow::Framebuffer,
    res_color: (glow::Framebuffer, glow::Texture),
    res_accum: (glow::Framebuffer, glow::Texture),
    res_reveal: (glow::Framebuffer, glow::Texture),
}

pub(super) struct Program {
    pub(super) program: glow::Program,
}

impl Program {
    pub(super) unsafe fn loc(&self, gl: &glow::Context, name: &str) -> Option<glow::UniformLocation> {
        gl.get_uniform_location(self.program, name)
    }
}

pub struct GlBackend {
    pub(super) gl: glow::Context,
    pub(super) sphere: Program,
    pub(super) bond: Program,
    pub(super) line: Program,
    pub(super) mesh_prog: Program,
    pub(super) section_prog: Program,
    composite: Program,
    pub(super) copy_prog: Program,
    pub(super) resolve_prog: Program,
    quad: glow::Buffer,
    sphere_vao: glow::VertexArray,
    sphere_vbo: glow::Buffer,
    bond_vao: glow::VertexArray,
    bond_vbo: glow::Buffer,
    line_vao: glow::VertexArray,
    line_vbo: glow::Buffer,
    section_vao: glow::VertexArray,
    pub(super) empty_vao: glow::VertexArray,
    n_atoms: i32,
    n_bonds: i32,
    n_lines: i32,
    line_color: [f32; 3],
    pub(super) line_width: f32,
    pub(super) meshes: Vec<Option<GpuMesh>>,
    ao_strength: f32,
    volume: Option<(glow::Texture, [usize; 3], [[f64; 3]; 3])>,
    section: Option<Section>,
    /// World-space clip plane (normal, d): keep normal·x <= d.
    clip: Option<([f32; 3], f32)>,
    pipeline: Option<Pipeline>,
    samples: i32,
}

pub(super) fn as_bytes<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: T is a `#[repr(C)]` struct of f32 (or an f32/u32 array) with no
    // padding; viewing it as bytes for upload is sound.
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

unsafe fn compile(gl: &glow::Context, vs: &str, fs: &str) -> Result<Program, String> {
    let program = gl.create_program()?;
    let mut shaders = Vec::new();
    for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
        let sh = gl.create_shader(kind)?;
        gl.shader_source(sh, src);
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
    Ok(Program { program })
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

/// Matrices for one draw, plus what clip and line uniforms need.
pub(super) struct Frame<'a> {
    pub(super) view: &'a [f32],
    pub(super) proj: &'a [f32],
    /// Clip plane in view space (normal, d), or zeros for none.
    pub(super) clip: [f32; 4],
    pub(super) viewport: (i32, i32),
}

impl GlBackend {
    /// Build programs and buffers. The GL context must be current.
    pub fn new(gl: glow::Context) -> Result<Self, String> {
        unsafe {
            let lit = |fs: &str| format!("#version 330 core\n{SHADE}{fs}");
            let sphere = compile(&gl, SPHERE_VS, &lit(SPHERE_FS))?;
            let bond = compile(&gl, BOND_VS, &lit(BOND_FS))?;
            let line = compile(&gl, LINE_VS, LINE_FS)?;
            let mesh_prog = compile(&gl, MESH_VS, &lit(MESH_FS))?;
            let section_prog = compile(&gl, SECTION_VS, SECTION_FS)?;
            let composite = compile(&gl, FULLSCREEN_VS, COMPOSITE_FS)?;
            let copy_prog = compile(&gl, FULLSCREEN_VS, COPY_FS)?;
            let resolve_prog = compile(&gl, FULLSCREEN_VS, RESOLVE_FS)?;

            // Fixed texture units per sampler.
            let bind_units = |p: &Program, units: &[(&str, i32)]| {
                gl.use_program(Some(p.program));
                for (name, unit) in units {
                    gl.uniform_1_i32(p.loc(&gl, name).as_ref(), *unit);
                }
            };
            bind_units(&composite, &[("opaque_tex", 0), ("accum_tex", 1), ("reveal_tex", 2)]);
            bind_units(&mesh_prog, &[("prev_depth", 3), ("opaque_depth", 4)]);
            bind_units(&section_prog, &[("volume", 5)]);
            bind_units(&copy_prog, &[("src", 0)]);
            bind_units(&resolve_prog, &[("opaque_tex", 0), ("accum_tex", 1)]);
            gl.use_program(None);

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
            instance_attribs(&gl, quad, line_vbo, &[3, 3]);

            let section_vao = gl.create_vertex_array()?;
            gl.bind_vertex_array(Some(section_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);

            let empty_vao = gl.create_vertex_array()?;
            let samples = gl.get_parameter_i32(glow::MAX_SAMPLES).clamp(0, 4);
            gl.bind_vertex_array(None);

            Ok(Self {
                gl,
                sphere,
                bond,
                line,
                mesh_prog,
                section_prog,
                composite,
                copy_prog,
                resolve_prog,
                quad,
                sphere_vao,
                sphere_vbo,
                bond_vao,
                bond_vbo,
                line_vao,
                line_vbo,
                section_vao,
                empty_vao,
                n_atoms: 0,
                n_bonds: 0,
                n_lines: 0,
                line_color: [0.3, 0.3, 0.3],
                line_width: 1.5,
                meshes: Vec::new(),
                ao_strength: 0.0,
                volume: None,
                section: None,
                clip: None,
                pipeline: None,
                samples,
            })
        }
    }

    /// The clip plane in view space for `view`.
    pub(super) fn clip_in_view(&self, view: &nalgebra::Matrix4<f32>) -> [f32; 4] {
        let Some((n, d)) = self.clip else { return [0.0; 4] };
        let r = view.fixed_view::<3, 3>(0, 0);
        let t = nalgebra::Vector3::new(view[(0, 3)], view[(1, 3)], view[(2, 3)]);
        let nv = r * nalgebra::Vector3::new(n[0], n[1], n[2]);
        [nv.x, nv.y, nv.z, d + nv.dot(&t)]
    }

    unsafe fn use_camera(&self, p: &Program, f: &Frame) {
        let gl = &self.gl;
        gl.use_program(Some(p.program));
        gl.uniform_matrix_4_f32_slice(p.loc(gl, "view").as_ref(), false, f.view);
        gl.uniform_matrix_4_f32_slice(p.loc(gl, "proj").as_ref(), false, f.proj);
        let c = f.clip;
        if let Some(l) = p.loc(gl, "clip") {
            gl.uniform_4_f32(Some(&l), c[0], c[1], c[2], c[3]);
        }
    }

    /// (Re)create the interactive offscreen targets for a `w`×`h` canvas.
    unsafe fn ensure_pipeline(&mut self, w: i32, h: i32) -> Result<(), String> {
        if self.pipeline.as_ref().is_some_and(|p| p.w == w && p.h == h) {
            return Ok(());
        }
        if let Some(old) = self.pipeline.take() {
            self.delete_pipeline(old);
        }
        let gl = &self.gl;
        let mut rbs = Vec::new();
        let mut fbos = Vec::new();
        let mut texs = Vec::new();
        let mut rb = |format: u32| -> Result<glow::Renderbuffer, String> {
            let r = gl.create_renderbuffer()?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(r));
            gl.renderbuffer_storage_multisample(glow::RENDERBUFFER, self.samples, format, w, h);
            rbs.push(r);
            Ok(r)
        };
        let color = rb(glow::RGBA8)?;
        let depth = rb(glow::DEPTH_COMPONENT24)?;
        let accum = rb(glow::RGBA16F)?;
        let reveal = rb(glow::R16F)?;

        let scene_fbo = gl.create_framebuffer()?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(scene_fbo));
        gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(color));
        gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::RENDERBUFFER, Some(depth));
        let oit_fbo = gl.create_framebuffer()?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(oit_fbo));
        gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(accum));
        gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT1, glow::RENDERBUFFER, Some(reveal));
        gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::RENDERBUFFER, Some(depth));
        fbos.push(scene_fbo);
        fbos.push(oit_fbo);

        let mut resolve = |internal: u32, format: u32, ty: u32| -> Result<(glow::Framebuffer, glow::Texture), String> {
            let t = new_texture(gl, internal, format, ty, w, h)?;
            let f = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(t), 0);
            texs.push(t);
            fbos.push(f);
            Ok((f, t))
        };
        let res_color = resolve(glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE)?;
        let res_accum = resolve(glow::RGBA16F, glow::RGBA, glow::HALF_FLOAT)?;
        let res_reveal = resolve(glow::R16F, glow::RED, glow::HALF_FLOAT)?;

        for f in [scene_fbo, oit_fbo] {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f));
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if status != glow::FRAMEBUFFER_COMPLETE {
                return Err(format!("offscreen framebuffer incomplete (0x{status:x})"));
            }
        }
        self.pipeline = Some(Pipeline {
            w,
            h,
            rbs,
            fbos,
            texs,
            scene_fbo,
            oit_fbo,
            res_color,
            res_accum,
            res_reveal,
        });
        Ok(())
    }

    unsafe fn delete_pipeline(&self, p: Pipeline) {
        for f in p.fbos {
            self.gl.delete_framebuffer(f);
        }
        for r in p.rbs {
            self.gl.delete_renderbuffer(r);
        }
        for t in p.texs {
            self.gl.delete_texture(t);
        }
    }

    /// Atoms, bonds, lines and the section plane into the bound framebuffer.
    pub(super) unsafe fn draw_primitives(&self, f: &Frame) {
        let gl = &self.gl;
        if self.n_atoms > 0 {
            self.use_camera(&self.sphere, f);
            gl.bind_vertex_array(Some(self.sphere_vao));
            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, self.n_atoms);
        }
        if self.n_bonds > 0 {
            self.use_camera(&self.bond, f);
            gl.bind_vertex_array(Some(self.bond_vao));
            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, self.n_bonds);
        }
        if self.n_lines > 0 {
            self.use_camera(&self.line, f);
            let p = &self.line;
            let c = self.line_color;
            gl.uniform_3_f32(p.loc(gl, "color").as_ref(), c[0], c[1], c[2]);
            gl.uniform_2_f32(p.loc(gl, "viewport").as_ref(), f.viewport.0 as f32, f.viewport.1 as f32);
            gl.uniform_1_f32(p.loc(gl, "width_px").as_ref(), self.line_width);
            gl.bind_vertex_array(Some(self.line_vao));
            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, self.n_lines);
        }
        if let (Some(s), Some((tex, dims, lat))) = (self.section, self.volume.as_ref()) {
            self.use_camera(&self.section_prog, f);
            let p = &self.section_prog;
            let m = nalgebra::Matrix3::new(
                lat[0][0], lat[1][0], lat[2][0], lat[0][1], lat[1][1], lat[2][1], lat[0][2], lat[1][2], lat[2][2],
            );
            if let Some(inv) = m.try_inverse() {
                let inv: nalgebra::Matrix3<f32> = inv.cast();
                gl.uniform_matrix_3_f32_slice(p.loc(gl, "inv_lattice").as_ref(), false, inv.as_slice());
                gl.uniform_3_f32(p.loc(gl, "origin").as_ref(), s.origin[0], s.origin[1], s.origin[2]);
                gl.uniform_3_f32(p.loc(gl, "e1").as_ref(), s.e1[0], s.e1[1], s.e1[2]);
                gl.uniform_3_f32(p.loc(gl, "e2").as_ref(), s.e2[0], s.e2[1], s.e2[2]);
                gl.uniform_3_f32(p.loc(gl, "dims").as_ref(), dims[0] as f32, dims[1] as f32, dims[2] as f32);
                gl.uniform_1_i32(p.loc(gl, "signed_map").as_ref(), s.signed as i32);
                gl.uniform_1_f32(p.loc(gl, "lo").as_ref(), s.lo);
                gl.uniform_1_f32(p.loc(gl, "hi").as_ref(), s.hi);
                gl.uniform_1_f32(p.loc(gl, "iso").as_ref(), s.iso);
                gl.active_texture(glow::TEXTURE5);
                gl.bind_texture(glow::TEXTURE_3D, Some(*tex));
                gl.bind_vertex_array(Some(self.section_vao));
                gl.disable(glow::CULL_FACE);
                gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
                gl.active_texture(glow::TEXTURE0);
            }
        }
    }

    /// Meshes of one kind (opaque or transparent) in `mode` (see MESH_FS).
    pub(super) unsafe fn draw_meshes(&self, f: &Frame, transparent: bool, mode: i32) {
        let gl = &self.gl;
        let p = &self.mesh_prog;
        self.use_camera(p, f);
        gl.uniform_1_i32(p.loc(gl, "mode").as_ref(), mode);
        gl.uniform_1_f32(p.loc(gl, "ao_strength").as_ref(), self.ao_strength);
        for m in self.meshes.iter().flatten() {
            if (m.material.opacity < 0.999) != transparent || m.count == 0 {
                continue;
            }
            let c = m.material.color;
            gl.uniform_3_f32(p.loc(gl, "color").as_ref(), c[0], c[1], c[2]);
            gl.uniform_1_f32(p.loc(gl, "opacity").as_ref(), m.material.opacity);
            gl.bind_vertex_array(Some(m.vao));
            gl.draw_elements(glow::TRIANGLES, m.count, glow::UNSIGNED_INT, 0);
        }
    }

    pub(super) fn any_transparent(&self) -> bool {
        self.meshes.iter().flatten().any(|m| m.material.opacity < 0.999 && m.count > 0)
    }

    /// Interactive mesh path: offscreen with MSAA and weighted blended OIT,
    /// then composited into the framebuffer GTK bound for us.
    unsafe fn draw_pipeline(&mut self, f: &Frame, bg: [f32; 3]) -> Result<(), String> {
        let (w, h) = f.viewport;
        let target = self.gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);
        let target = std::num::NonZeroU32::new(target as u32).map(glow::NativeFramebuffer);
        self.ensure_pipeline(w, h)?;
        let gl = &self.gl;
        let p = self.pipeline.as_ref().expect("just ensured");
        let any_transparent = self.any_transparent();

        // Opaque: atoms, bonds, lines, section, opaque meshes.
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(p.scene_fbo));
        gl.draw_buffers(&[glow::COLOR_ATTACHMENT0]);
        gl.viewport(0, 0, w, h);
        gl.clear_color(bg[0], bg[1], bg[2], 1.0);
        gl.depth_mask(true);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LESS);
        gl.disable(glow::BLEND);
        self.draw_primitives(f);
        self.draw_meshes(f, false, 0);

        // Transparent: depth-tested against the opaque scene, never written.
        if any_transparent {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(p.oit_fbo));
            gl.draw_buffers(&[glow::COLOR_ATTACHMENT0, glow::COLOR_ATTACHMENT1]);
            gl.clear_buffer_f32_slice(glow::COLOR, 0, &[0.0, 0.0, 0.0, 0.0]);
            gl.clear_buffer_f32_slice(glow::COLOR, 1, &[1.0, 1.0, 1.0, 1.0]);
            gl.depth_mask(false);
            gl.enable(glow::BLEND);
            gl.draw_buffers(&[glow::COLOR_ATTACHMENT0, glow::NONE]);
            gl.blend_func(glow::ONE, glow::ONE);
            self.draw_meshes(f, true, 1);
            gl.draw_buffers(&[glow::NONE, glow::COLOR_ATTACHMENT1]);
            gl.blend_func(glow::ZERO, glow::ONE_MINUS_SRC_COLOR);
            self.draw_meshes(f, true, 2);
            gl.disable(glow::BLEND);
            gl.depth_mask(true);
        }

        // Resolve the multisampled targets.
        let blit = |from: glow::Framebuffer, attachment: u32, to: glow::Framebuffer| {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(from));
            gl.read_buffer(attachment);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(to));
            gl.draw_buffers(&[glow::COLOR_ATTACHMENT0]);
            gl.blit_framebuffer(0, 0, w, h, 0, 0, w, h, glow::COLOR_BUFFER_BIT, glow::NEAREST);
        };
        blit(p.scene_fbo, glow::COLOR_ATTACHMENT0, p.res_color.0);
        if any_transparent {
            blit(p.oit_fbo, glow::COLOR_ATTACHMENT0, p.res_accum.0);
            blit(p.oit_fbo, glow::COLOR_ATTACHMENT1, p.res_reveal.0);
        }

        // Composite into GTK's framebuffer.
        gl.bind_framebuffer(glow::FRAMEBUFFER, target);
        gl.viewport(0, 0, w, h);
        gl.disable(glow::DEPTH_TEST);
        gl.use_program(Some(self.composite.program));
        gl.uniform_1_i32(self.composite.loc(gl, "has_transparent").as_ref(), any_transparent as i32);
        for (unit, tex) in [p.res_color.1, p.res_accum.1, p.res_reveal.1].into_iter().enumerate() {
            gl.active_texture(glow::TEXTURE0 + unit as u32);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        }
        gl.bind_vertex_array(Some(self.empty_vao));
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        gl.active_texture(glow::TEXTURE0);
        Ok(())
    }
}

/// A 2D texture with nearest filtering.
pub(super) unsafe fn new_texture(gl: &glow::Context, internal: u32, format: u32, ty: u32, w: i32, h: i32) -> Result<glow::Texture, String> {
    let t = gl.create_texture()?;
    gl.bind_texture(glow::TEXTURE_2D, Some(t));
    gl.tex_image_2d(glow::TEXTURE_2D, 0, internal as i32, w, h, 0, format, ty, glow::PixelUnpackData::Slice(None));
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
    Ok(t)
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
        // Pairs of points to one instance per segment.
        let inst: Vec<[f32; 6]> = segments
            .chunks_exact(2)
            .map(|s| [s[0][0], s[0][1], s[0][2], s[1][0], s[1][1], s[1][2]])
            .collect();
        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.line_vbo));
            self.gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(&inst), glow::STREAM_DRAW);
        }
        self.n_lines = inst.len() as i32;
        self.line_color = color;
    }

    fn set_line_width(&mut self, px: f32) {
        self.line_width = px.max(0.5);
    }

    fn set_mesh(&mut self, slot: usize, mesh: Option<MeshData>) {
        if self.meshes.len() <= slot {
            self.meshes.resize_with(slot + 1, || None);
        }
        let gl = &self.gl;
        let material = self.meshes[slot].as_ref().map(|m| m.material).unwrap_or(Material {
            color: [0.9, 0.8, 0.2],
            opacity: 1.0,
        });
        unsafe {
            if let Some(old) = self.meshes[slot].take() {
                gl.delete_vertex_array(old.vao);
                gl.delete_buffer(old.vbo);
                gl.delete_buffer(old.ebo);
            }
            let Some(m) = mesh else { return };
            let mut interleaved: Vec<[f32; 7]> = Vec::with_capacity(m.positions.len());
            for (i, (p, n)) in m.positions.iter().zip(m.normals).enumerate() {
                let ao = m.ao.and_then(|a| a.get(i)).copied().unwrap_or(1.0);
                interleaved.push([p[0], p[1], p[2], n[0], n[1], n[2], ao]);
            }
            let (Ok(vao), Ok(vbo), Ok(ebo)) = (gl.create_vertex_array(), gl.create_buffer(), gl.create_buffer()) else {
                return;
            };
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(&interleaved), glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 28, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, 28, 12);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 1, glow::FLOAT, false, 28, 24);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, as_bytes(m.indices), glow::STATIC_DRAW);
            gl.bind_vertex_array(None);
            self.meshes[slot] = Some(GpuMesh { vao, vbo, ebo, count: m.indices.len() as i32, material });
        }
    }

    fn set_mesh_material(&mut self, slot: usize, material: Material) {
        if let Some(Some(m)) = self.meshes.get_mut(slot) {
            m.material = material;
        }
    }

    fn set_ao_strength(&mut self, strength: f32) {
        self.ao_strength = strength.clamp(0.0, 1.0);
    }

    fn set_volume(&mut self, volume: Option<Volume>) {
        let gl = &self.gl;
        unsafe {
            if let Some((t, _, _)) = self.volume.take() {
                gl.delete_texture(t);
            }
            let Some(v) = volume else { return };
            // Halve the resolution until the grid fits the 3D texture limit.
            let max = gl.get_parameter_i32(glow::MAX_3D_TEXTURE_SIZE).max(64) as usize;
            let step = (0..6).map(|k| 1usize << k).find(|s| v.dims.iter().all(|d| d.div_ceil(*s) <= max)).unwrap_or(32);
            let dims = v.dims.map(|d| d.div_ceil(step));
            let mut data = Vec::with_capacity(dims[0] * dims[1] * dims[2]);
            for k in 0..dims[2] {
                for j in 0..dims[1] {
                    for i in 0..dims[0] {
                        let (x, y, z) = (i * step, j * step, k * step);
                        data.push(v.data[x + v.dims[0] * (y + v.dims[1] * z)]);
                    }
                }
            }
            let Ok(t) = gl.create_texture() else { return };
            gl.bind_texture(glow::TEXTURE_3D, Some(t));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_image_3d(
                glow::TEXTURE_3D,
                0,
                glow::R32F as i32,
                dims[0] as i32,
                dims[1] as i32,
                dims[2] as i32,
                0,
                glow::RED,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(as_bytes(&data))),
            );
            for (p, val) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::REPEAT),
                (glow::TEXTURE_WRAP_T, glow::REPEAT),
                (glow::TEXTURE_WRAP_R, glow::REPEAT),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_3D, p, val as i32);
            }
            gl.bind_texture(glow::TEXTURE_3D, None);
            self.volume = Some((t, dims, v.lattice));
        }
    }

    fn set_section(&mut self, section: Option<Section>) {
        self.section = section;
    }

    fn set_clip(&mut self, clip: Option<([f32; 3], f32)>) {
        self.clip = clip;
    }

    fn draw(&mut self, camera: &Camera, width: i32, height: i32, bg: [f32; 3]) {
        let view = camera.view();
        let proj = camera.projection(width as f64, height as f64);
        let frame = Frame {
            view: view.as_slice(),
            proj: proj.as_slice(),
            clip: self.clip_in_view(&view),
            viewport: (width, height),
        };
        unsafe {
            if self.meshes.iter().any(Option::is_some) || self.section.is_some() {
                if let Err(e) = self.draw_pipeline(&frame, bg) {
                    crate::utils::console::log_error(&format!("3D view: {e}"));
                }
            } else {
                let gl = &self.gl;
                gl.viewport(0, 0, width, height);
                gl.clear_color(bg[0], bg[1], bg[2], 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                gl.enable(glow::DEPTH_TEST);
                gl.depth_func(glow::LESS);
                self.draw_primitives(&frame);
            }
            self.gl.bind_vertex_array(None);
            self.gl.use_program(None);
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
        unsafe {
            let gl = &self.gl;
            for p in [
                &self.sphere,
                &self.bond,
                &self.line,
                &self.mesh_prog,
                &self.section_prog,
                &self.composite,
                &self.copy_prog,
                &self.resolve_prog,
            ] {
                gl.delete_program(p.program);
            }
            for b in [self.quad, self.sphere_vbo, self.bond_vbo, self.line_vbo] {
                gl.delete_buffer(b);
            }
            for v in [self.sphere_vao, self.bond_vao, self.line_vao, self.section_vao, self.empty_vao] {
                gl.delete_vertex_array(v);
            }
            for m in self.meshes.drain(..).flatten() {
                gl.delete_vertex_array(m.vao);
                gl.delete_buffer(m.vbo);
                gl.delete_buffer(m.ebo);
            }
            if let Some((t, _, _)) = self.volume.take() {
                gl.delete_texture(t);
            }
        }
        if let Some(p) = self.pipeline.take() {
            unsafe { self.delete_pipeline(p) };
        }
    }
}
