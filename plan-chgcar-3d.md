# Plan: 3D charge density (CHGCAR) — interactive view and publication export

## Goal

The 3D charge-density view is CView's flagship feature. It should beat VESTA on three counts:

1. **Picture quality:** surfaces, transparency and depth cues that read clearly in print.
2. **Ease of use:** a good isovalue on the first try, and live, responsive controls.
3. **Export:** a figure that meets Nature/Science artwork specifications directly, with no Photoshop step.

It lives in the existing Charge Density window (`ui/analysis/charge_density_tab.rs`) as a 3D page next to the 2D slice view, and reuses its loaded `ChgcarData`: total, magnetisation, up/down, and A−B difference. It renders through `rendering/gl` (OpenGL 3.3 core). Unlike the trajectory player, **quality wins over speed** here, and it can afford to: the scene is one static mesh plus a few atoms.

## What journals ask for (final artwork)

| | Nature | Science* |
|---|---|---|
| Widths | 89 mm (1 col), 183 mm (2 col) | 5.5 cm, 12 cm, 17.5 cm |
| Max height | 170 mm | — |
| Rendered/photographic images | ≥ 300 dpi; ≥ 450 dpi gives the best online proofs | ≥ 300 dpi |
| Images combined with line art or text | 600 dpi | — |
| Line art | 1000–1200 dpi, or vector | vector preferred |
| Text | sans-serif (Helvetica/Arial), 5–7 pt at final size | — |
| Colour | RGB (converted to CMYK for print) | — |

\*Science numbers are from secondary summaries; check against the current Science author guide before release.

The consequences for export:
- **Render at final physical size and dpi.** Never render at screen size and scale up.
- **The image and the text are different things:**
  - The surface and atom image is raster, at 600 dpi by default (it is "combined" artwork once labels are added).
  - Labels, colour bar, axes and scale bar are **vector**, so they stay sharp and editable in Illustrator/Inkscape.
- **Line widths are physical** (in points), not pixels. A 1-pixel line is invisible at 600 dpi.

## Export design

### 1. Figure mode

A "Figure…" dialog previews the exact output:
- **Size:** preset (Nature 1 col / 2 col, Science 1 / 1.5 / 2 col, ACS, custom) in mm, or pixels for slides.
- **Resolution:** 300 / 450 / **600** (default) / 1200 dpi.
- **Background:** white, or transparent for compositing into multi-panel figures.
- **Annotations, each optional and all vector:**
  - colour bar,
  - isovalue (e.g. "±0.005 e Å⁻³"),
  - axis triad with a/b/c labels,
  - scale bar in Å,
  - atom labels,
  - panel letter (a, b, c…).

  Font defaults to Helvetica/Arial (falling back to DejaVu Sans), at 6 pt **at final size**.

The preview is drawn at the final aspect ratio, so what you frame is what you get.

### 2. Rendering the raster layer off-screen

- **Output size:** at final size, e.g. 183 mm at 600 dpi = 4323 px wide.
- **Supersampling:** render at 3× (SSAA 3×3 on top of the hardware MSAA), then downsample with a proper filter (Lanczos-3 or box over linear-light values). Edges come out smooth even where an isosurface meets an atom.
- **Tiling:** split into tiles when the target exceeds `GL_MAX_RENDERBUFFER_SIZE` (often 16384). That happens with 183 mm at 1200 dpi with 3× supersampling, for example. Each tile shifts the projection; tiles are stitched on the CPU, so any size works on any GPU.
- **Exact transparency for export:** depth peeling, at 8–12 layers. The interactive view uses the fast approximate method (weighted blended OIT); export uses the exact one, so nested and overlapping transparent lobes are drawn correctly.
- **Colour:** linear-light shading converted to sRGB, with an sRGB chunk/ICC tag, dithered to 8 bits so smooth gradients don't band. 16-bit output is offered for those who post-process.
- **Physical sizes:** line widths (cell edges, bonds outline) are given in pt, converted to pixels at the export dpi, and drawn as geometry, so they are depth-correct and the same in the preview as in the file.

### 3. Assembling the file

- **PDF / SVG (default for journals):** Cairo page at the exact physical size. It contains:
  - the supersampled raster at 600 dpi;
  - **vector** annotations on top, in embedded fonts.

  Annotations that belong to 3D points (atom labels, axis triad) are projected with the same camera matrices. They are hidden by testing against the read-back depth buffer, so a label behind a lobe is not drawn.
- **TIFF (LZW) / PNG:** with the dpi written into the file (TIFF resolution tags, PNG `pHYs`). The annotations are rasterised at the same dpi.
- **Separate layers (option):** isosurface, atoms and annotations as separate transparent PNGs plus the vector PDF, for people who compose panels by hand.

### 4. Reproducibility and other renderers

- **Scene file:** every export writes `figure.cview.json`: camera, isovalues, colours, materials, size/dpi, and file hashes of the CHGCARs. "Open scene" restores it exactly, so a referee-requested revision is a 30-second job.
- **Mesh export:** OBJ/PLY with normals and colours, and glTF with materials, for Blender or POV-Ray users.

## Making it better than VESTA

### Quality
- **Smooth, seamless surfaces:**
  - Marching cubes on the periodic grid, with vertices shared between neighbouring cubes, so normals are smooth and the mesh is small.
  - Normals from the analytic gradient of the trilinear interpolant, not face normals, so surfaces are smooth even on coarse grids.
  - The surface is seamless across cell faces.
- **Depth cues that survive print:** ambient occlusion computed once per mesh (rays against the mesh, on a worker), because the mesh is static. This gives much better occlusion than screen-space AO. It is applied to atoms too, and an optional soft key-light shadow is added for export.
- **Proper transparency:** see Export. Lobes keep a solid silhouette, with Fresnel-weighted opacity, so they read as shapes rather than haze.
- **Cut-aways with capped cross-sections:** a clip plane or slab (by hkl or by the current 2D slice), where the cut face is **filled and coloured by density** with contour lines. VESTA leaves cut surfaces hollow.
- **The slice inside the 3D view:** the 2D slice already computed in this window, shown as a textured plane in the 3D scene, so slice and isosurface can be read together.
- **Atoms and bonds:** they use the main view's colours, radii and materials (ported into the shader for this window), with the same boundary images. A displayed range beyond one cell (e.g. 2×2×1), or "atoms within X Å of the surface", as VESTA's boundary option does.

### Physics and ease of use
- **Correct units throughout:** e Å⁻³; `ChgcarData` already divides by the cell volume.
- **Isovalue picking:**
  - A histogram of |ρ| next to the slider, with the isovalue marked.
  - An **"enclose X% of the charge"** mode (e.g. 90% of the electrons inside the surface), which is comparable across systems in a way an absolute isovalue is not.
  - A sensible automatic default per channel: total density, difference (symmetric ±), spin density (±, up/down colours).
- **Live slider:** the mesh is rebuilt on a worker (`utils::task::spawn`, cancelled on each new value). The rebuilt surface appears in tens of ms for 100–200³ grids; while waiting, the previous mesh stays on screen.
- **Report** the charge enclosed by the surface and its volume, written to Structure Info.
- **Presets:** "Bonding (difference)", "Spin density", "Total density", each with colours and isovalue logic.
- **Views:** along a/b/c, along [uvw], normal to (hkl), and "match main view" (copy the orientation).
- **Same controls as the main view:** same mouse behaviour, plus orthographic/perspective.

### Large grids
- **Memory:** `ChgcarData` holds `f64`, so a 400³ grid takes 512 MB per channel. The 3D page works from an `f32` copy (half the memory), and later the parser should read straight into `f32`.
- **Parsing:** CHGCAR parsing moves onto a worker; it currently runs on the UI thread.
- **Meshing:** chunked marching cubes with `rayon`, so meshing a 400³ grid takes well under a second on 8 cores. The GPU holds a few million triangles easily.

## GL backend additions (`rendering/gl`)

- `upload_mesh(id, vertices: pos+normal+AO, indices: u32, material)`, `remove_mesh(id)`.
- A material model (base colour, opacity, metallic, roughness, Fresnel), shared by the meshes and the atom/bond shaders of this window.
- **Passes:**
  1. opaque (atoms, bonds, opaque surfaces);
  2. transparent (weighted blended OIT interactively, depth peeling for export);
  3. thick lines (cell edges) as screen-aligned quads in pt;
  4. clip-plane caps via stencil.
- **Off-screen targets:** an MSAA FBO resolved to a texture, tiled rendering, and `read_pixels` of float or 8-bit data.
- **Timer queries** (`GL_TIME_ELAPSED`, core in 3.3) to keep the interactive view within its frame budget. Export ignores the budget.

## Status

**Phase A done.** It lives in `src/physics/analysis/isosurface.rs`.
- **Method:** marching *tetrahedra* (Freudenthal split) instead of marching cubes. There are no ambiguous cases, so the surface is watertight by construction, which the tests check.
- **Crossings on grid points:** a crossing that lands exactly on a grid point is shared, so there are no coincident vertices and no zero-area triangles.
- **Accuracy:**
  - sphere volume and area within 1% (orthorhombic and triclinic cells);
  - normals outward;
  - seamless across cell faces;
  - the saddle-heavy cos+cos+cos field gives a closed surface with area 2.345 a².
- **Speed:**
  - BaTiO3 60³: 3–10 ms;
  - 200³ synthetic with 2.5M triangles: 155 ms;
  - 300³ synthetic with 5.6M triangles: 0.4 s (18 threads).

**Phase B done.** "3D Isosurface" page in the Charge Density window, in `src/ui/analysis/charge_density_3d.rs`.
- **Data:** it follows the files, difference mode and channel chosen on the right pane. The page polls a fingerprint of those settings. Every control applies immediately; the sliders that shape the picture live in a bottom bar that switches with the page.
- **Isovalue:** a logarithmic slider plus an exact spin box in e/Å³. The default is the 95th percentile of |ρ|.
- **Lobes:** +, − or ±, picked automatically from the sign range.
- **Appearance:** opacity, two lobe colours, atoms (with face images) and cell.
- **Views:** true lattice directions along a/b/c (correct for triclinic cells), plus Fit.
- **Meshing:** on a worker; each slider move replaces the running job.
- **GL backend:** gained indexed meshes and an offscreen pipeline: 4× MSAA, two-pass weighted blended OIT (GL 3.3 has no per-target blend functions), then a composite into GTK's framebuffer. The trajectory player still draws directly, as before.
- **New entry point:** `window::show_charge_density_window_with(parent, state, Some(path))` opens the window with a file already loaded. It is not yet wired to File → Open.

**Phases C, D and E done.**

C — export:
- **Figure dialog:** journal width presets, size in mm, 300/450/600/1200 dpi, 2–4× supersampling, white or transparent background, font and line sizes in pt, panel letter, and checkboxes for title, isovalue key, colour bar, axes, scale bar and atom labels.
- **Live readout:** pixel size, plus warnings against Nature's 170 mm height and 5–7 pt text limits.
- **Framing:** the 3D view shows the picture frame and the dialog fits the structure into it. Annotations go in reserved bands above and below, so they never cover the structure.
- **Rendering** (`gl/export.rs`): tiled (any size on any GPU; 183 mm at 1200 dpi is 8646 × 6052 px in 4.3 s), supersampled with box filtering in linear light, dithered, with depth-peeled exact transparency.
- **Output** (`rendering/figure.rs`): PDF/SVG at the exact page size (pdfinfo: 89 × 70 mm → 252.283 × 198.425 pt) with the raster embedded losslessly at the chosen ppi and vector text in embedded fonts; PNG with pHYs and sRGB; TIFF with X/YResolution.
- **Scene file:** written next to every figure.

D — quality:
- **Ambient occlusion:** field-based, from rays marched through the density, with atoms as occluders. It is computed per vertex on the meshing worker.
- **Exact transparency for exports:** depth peeling.
- **Section plane by (hkl) and position:** coloured by density from a 3D texture, using log-viridis for densities and a diverging map saturating at ±iso for difference/spin densities. Contours sit at the isovalue.
- **Cut-away:** a clip plane removes surfaces, bonds and whole atoms beyond the plane; the section plane then serves as the filled, density-coloured cut face.

E — ease of use:
- **Histogram** of |ρ| under the canvas; click it to set the isovalue.
- **"Enclosed charge" mode:** the isovalue enclosing X% of the charge. Checked: 50% gives 19.0014 of 38 e.
- **Enclosed charge in the HUD and Report:** electrons and volume per lobe, and the net ∫ρ dV (38.0000 e for BaTiO3).
- **Presets:** total, bonding/difference ±, and spin density (which switches the channel to magnetisation).
- **Scene save/load:** with a data hash, warning on mismatch.
- **Mesh export:** OBJ+MTL and PLY.

Differs from the plan / not done:
- **No stencil caps:** the cut face is the density section plane.
- **AO:** field-based rather than ray/triangle.
- **Not done:**
  - glTF export;
  - 16-bit output;
  - "match main view" orientation;
  - atom labels hidden by lobes (only atoms hide labels);
  - volume ray-marching;
  - a displayed range beyond one cell;
  - CHGCAR parsing on a worker and as f32 (large-grid item).
- **Science width presets** still need checking against Science's own guide.

## Phases

| # | Work | Done when |
|---|---|---|
| A | Isosurface extraction (periodic, shared vertices, gradient normals) + tests | Enclosed volume of a sphere-like analytic field matches voxel count within 1%; seamless across cell faces |
| B | 3D page: mesh + atoms + cell, isovalue slider, ± lobes, transparency (WBOIT), worker remeshing | `BaTiO3_O-def − pure` shows ± lobes around the vacancy; slider responsive on a 200³ grid |
| C | Figure dialog: physical size presets, off-screen SSAA + tiling, PDF/SVG with vector annotations, TIFF/PNG with dpi | A 183 mm / 600 dpi PDF passes Nature's size/dpi/font rules; text is selectable vector |
| D | Quality: baked AO, depth peeling for export, clip plane with filled caps, slice plane in 3D | Side-by-side with VESTA at the same isovalue: clearer depth and cut faces |
| E | Ease of use: histogram, enclose-X% mode, presets, enclosed-charge report, scene file, mesh export | Reproducing a saved figure is one click |

Phases A to C give a publishable figure. D and E are what put it clearly ahead of VESTA.

## Verification
- **Unit tests:** marching cubes on analytic fields (sphere, periodic cosine) for volume, watertightness and normals; "enclose X%" against direct summation.
- **Checks on the BaTiO3 pair:** ±ρ symmetry of the difference, enclosed charge consistent with electron counts.
- **Export tests:**
  - the PDF page size equals the preset in mm;
  - the embedded image's pixel size equals size × dpi;
  - the text is vector;
  - a 1200 dpi double-column export completes through tiling.
- **Visual review** against VESTA output for the same files and isovalues.
