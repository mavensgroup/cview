// src/rendering/gl.rs
//
// OpenGL 3.3 core renderer for the GPU windows (trajectory player, and later
// the 3D charge density view). The main structure view stays on Cairo.
// See plan-gpu-rendering.md.
//
// Everything above this module talks to `backend::GpuBackend`, so the GL
// implementation can be replaced (e.g. by wgpu) without touching the windows.

pub mod backend;
pub mod camera;
pub mod export;
pub mod loader;

pub use backend::{AtomInstance, BondInstance, GlBackend, GpuBackend, Material, MeshData, Section, Volume};
pub use export::{ExportRequest, RgbaImage};
pub use camera::Camera;
