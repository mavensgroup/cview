# Plan: GPU rendering for MD playback and 3D charge density

## Status (2026-10-03)

Done:
- **Phase 1:** GL core (`src/rendering/gl/`).
- **Phase 3, partly:** the trajectory player (`src/ui/trajectory_player.rs`). Opened from **Structure → Trajectory Player…** (Ctrl+Shift+P) rather than Analysis, because "Show in Main View" changes the structure.
- **Phase 5, partly:** multi-frame readers for vasprun.xml, QE output and LAMMPS dumps. Preferences → General picks whether the first or last frame opens.

Differs from the plan:
- Frames are held as `f64` (`model::trajectory`), not `f32`.
- Whole files are parsed on a worker (`utils::task`), without the byte-offset index or LRU.
- The `GpuBackend` interface takes the full instance list per frame (`set_atoms`/`set_bonds`/`set_lines`), so boundary images can change between frames.

Not started:
- Phase 2: CHGCAR 3D.
- Phase 4: colour-by, movie export.
- XDATCAR and multi-frame XYZ readers.
- Indexed and lazy loading for very large dumps.

## Decision

The **main viewport stays Cairo**: no change to `scene.rs`, `painter.rs`, the export dialog or the sidebar. The GPU is used only in **two separate windows**, in the same way charge density already works:

| Window | Opened from | Today | Adds |
|---|---|---|---|
| Charge Density | Analysis → Charge Density (`ui/analysis/window.rs::show_charge_density_window`) | 2D slices + isolines (`charge_density_tab.rs`) | A **3D** view: isosurfaces + atoms + cell |
| Trajectory | Analysis → Trajectory (new) | none | A GPU **player** for multi-frame files |

Both windows share one small GL module. The main view keeps vector PDF/SVG export for publication figures, which is where single frames come from anyway.

## Why

- **Speed (measured, headless, release build, Intel Arc).** For 1500 atoms, `calculate_scene` takes 2.3 ms and Cairo `draw_structure` takes 95 ms per frame, about 10 fps. That is for the smallest MD case in `excmples/`, and real LAMMPS runs have 10^5 to 10^6 atoms.
- **Isosurfaces** need a depth buffer and transparency, which Cairo has neither of.
- **Separate windows avoid** the painter refactor, a GPU/CPU fallback inside the main view, and look-parity tests between the two backends. Nothing that works today is touched.

## Graphics API: OpenGL 3.3 core (decided)

**Choice:** `GtkGLArea` with the `glow` crate, OpenGL 3.3 core profile, GLSL `#version 330 core`. Function pointers come from libepoxy, which GTK already links.

**Why OpenGL despite the macOS deprecation:**
- GTK 4 on macOS renders its own UI through OpenGL by default. If Apple removed OpenGL, all of GTK (and CView's whole UI) would break first, and the fix would come with GTK. Using GL in our windows adds no risk that we don't already carry.
- Apple deprecated OpenGL in 2018 and has not removed it. On Apple Silicon it runs on top of Metal.
- `GtkGLArea` is GTK's native path: rendering goes straight into the widget with no per-frame copy, and the dependency is small.
- It works in all four builds today: Flatpak (`--device=dri` already set), Windows/MSYS2, macOS (GL 4.1 core, so 3.3 core is supported) and DEB/RPM.

**Considered and rejected for now: wgpu** (Metal, Vulkan, DX12). GTK cannot show a wgpu surface directly. Each frame would be read back to the CPU (about 3 MB at 1000x800) and handed to GTK as a texture. That adds a frame of latency, stalls on discrete GPUs, and brings a heavy dependency into all four packaging targets. Revisit only if GTK itself moves off OpenGL on macOS.

**Keeping the exit open:** all GPU calls live behind one small interface in `src/rendering/gl/backend.rs`:

```
trait GpuBackend {
    fn upload_atoms(&mut self, atoms: &[AtomInstance]);   // pos, radius, rgba
    fn update_positions(&mut self, pos: &[[f32; 3]]);     // per trajectory frame
    fn upload_mesh(&mut self, id: MeshId, mesh: &Mesh);   // isosurfaces, cell
    fn draw(&mut self, camera: &Camera, size: (u32, u32));
    fn draw_offscreen(&mut self, camera: &Camera, size: (u32, u32)) -> RgbaImage; // PNG/movie
}
```

The windows, `Camera`, marching cubes, frame storage and the Cairo text overlay use only this interface. Swapping to wgpu would mean rewriting the shaders and buffer setup behind it, not the windows.

**Rules for the GL code:**
- Core profile 3.3 only: no compute shaders, no geometry shaders, no extensions. Instancing, framebuffer objects and float textures are all core in 3.3.
- Call `GLArea::set_required_version(3, 3)`. On macOS, GTK provides a forward-compatible core context; do not use deprecated fixed-function calls.
- All GL objects are created in `realize` and freed in `unrealize`. GL calls happen only inside `render` or after `make_current`.
- Check the context in `realize`. On failure, show the message in the window and log it to the System Log; don't panic.
- Run the shaders through a headless CI check (`glslangValidator` if available) so a typo can't reach a macOS build untested.

## Shared GL core: `src/rendering/gl/`

- **Widget:** `GtkGLArea` + `glow`, as decided above. New crate dependencies are `glow` and `epoxy`; there are no new system packages.
- **`Camera`:** orthographic projection with a quaternion trackball, zoom and pan, using the same drag feel as `interactions.rs` (`DRAG_ROTATE_DEG_PER_PX`).
- **Atoms:** instanced sphere impostors, ray-cast in the fragment shader with `gl_FragDepth`. One quad per atom, so a million atoms is fine. The shading constants are taken from `create_atom_sprite` so colours match the main view.
- **Bonds and cell:** cylinder impostors for bonds (optional in the player) and lines for the cell.
- **Meshes:** triangle meshes with weighted-blended order-independent transparency, for isosurfaces.
- **Overlay:** text and HUD are drawn with Cairo on a transparent `DrawingArea` stacked with `gtk::Overlay` (frame counter, colour legend, axis triad).
- **Offscreen render:** to a framebuffer, then a PNG at a chosen DPI. Both windows use this for export.
- **Failure handling:** if the GL context fails (VM, remote X), the window shows a clear message and logs it to the System Log. The main view is unaffected.

## Window 1: Charge Density → 3D tab

- Keep the 2D slice UI. Add a notebook or toggle for a "3D" page that reuses the window's loaded `ChgcarData`: the total, the magnetisation, and the A−B difference already supported there.
- **Mesh:** marching cubes on the periodic grid, wrapped or clipped to the cell, with normals from the density gradient. It runs on `rayon` inside `utils::task::spawn`. The 60³ `BaTiO3_*.CHGCAR` takes milliseconds; 300³ takes about 1 s, so moving the isovalue slider cancels and replaces the running job.
- **Signs:** the ± lobes get two colours with adjustable opacity, which suits difference and spin density.
- **Atoms:** taken from the CHGCAR header, as the 2D view already does.
- **Export:** high-DPI PNG. Isosurface figures are raster in every tool.
- **Later:** a textured slice plane in 3D, and volume ray-marching.

## Window 2: Trajectory player (replaces the UI part of `plan-animation.md`)

### Data
- **Parsing and storage:** `io::lammps_dump` grows from first-frame-only to all frames. Frames are stored compactly (`f32` positions per frame, plus a shared type/species table, the box, and step/time). This replaces the plan's `Vec<Structure>`: the 2041-frame run is about 37 MB this way instead of several hundred.
- **Large files:** frame byte offsets are indexed on a worker and frames decoded on demand through an LRU. The full parse of the 94 MB dump took 0.6 s earlier.
- **Caching:** the parsed trajectory is kept as an `Rc` on the tab, like `kpath_result`, so reopening the window does not re-parse. The window itself owns the playback state (frame index, fps, loop, colour-by).
- **Opening a dump:** this stays as now. The main tab shows frame 0, and the System Log says "N frames, open Analysis → Trajectory to play".

### Player
- **Controls:** play/pause, step, first and last, a frame slider, fps, loop, and wrapped/unwrapped positions.
- **Unwrapping:** uses `xu` or image flags when present, otherwise minimum-image reconstruction. This was verified exact against the `xu` file earlier.
- **Per frame:** positions are uploaded to the instance buffer (18 KB for 1500 atoms) and redrawn. No `Structure` is built and nothing is invalidated. The view is framed by the cell, so the zoom does not pulse.
- **Colour by:** species, displacement, speed, coordination and any dump column, as a per-instance colour buffer with a legend.
- **Back to the main view:** a "Send frame to main view" button builds a `Structure` for the current frame and opens it as a tab, so measurements, BVS, symmetry and export all work on it. This is the only connection to the Cairo side.
- **Movie:** offscreen frames to a PNG sequence, then `ffmpeg` if it is on PATH (MP4/WebM).
- **Plots (later):** MSD and g(r) on `plotters`, like the XRD tab.

## Phases

| # | Work | Done when |
|---|---|---|
| 1 | GL core: context, camera, sphere impostors, cell lines, overlay, offscreen PNG | A test window rotates 100k spheres at 60 fps on this Intel Arc |
| 2 | CHGCAR 3D tab (marching cubes, ± lobes, atoms) | `BaTiO3_O-def` minus `pure` shows lobes; the slider stays responsive |
| 3 | Multi-frame dump reading (index + LRU) and trajectory window | `col5-100k-wrap` plays smoothly; the 94 MB dump opens without blocking |
| 4 | Colour-by, send frame to main view, movie export | — |
| 5 | Other formats from `plan-animation.md` (QE relax, XDATCAR, multi-frame XYZ) | Each feeds the same player |

Phases 2 and 3 are independent once phase 1 is done.

## Risks and trade-offs

- **Interaction does not carry over:** picking, measurements and polyhedra stay in the main view. "Send frame to main view" covers that for trajectories.
- **Look:** GPU windows may look slightly different from the main view. Shared shading constants keep this small, and it matters less in separate windows.
- **macOS:** OpenGL is deprecated there but works, and GTK itself depends on it. If that ever changes, replace the `GpuBackend` implementation with wgpu (see "Graphics API").
- **Old or virtual GPUs without GL 3.3:** the windows show a message. The main Cairo view and all non-3D features keep working.
- **Not done:** a GPU main viewport. It stays possible later (the GL core would be reused), but nothing here needs it.
