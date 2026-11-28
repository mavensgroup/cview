# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CView is a Rust + GTK4 desktop application for crystal structure visualization and analysis aimed at *ab-initio* DFT workflows (VASP, Quantum Espresso, SPR-KKR). Rendering uses CPU-based Cairo (no GPU dependency). Symmetry detection comes from `moyo`; linear algebra from `nalgebra`.

## Common Commands

```bash
# Run the app (from repo root, not src/)
cargo run --release

# Run with a structure file pre-loaded
cargo run --release -- BaTiO3.cif

# Debug a startup crash
RUST_BACKTRACE=1 cargo run

# Build a release binary
cargo build --release

# Lint check (the crate sets unused_imports/unused_variables to warn)
cargo check
cargo clippy
```

System dependencies (Fedora): `sudo dnf install gcc gtk4-devel libadwaita-devel fontconfig-devel freetype-devel` (EL9: enable CRB first, for `libadwaita-devel`). Minimum system versions are GTK 4.4, libadwaita 1.4, pango 1.46, set by the `v4_4`/`v1_4` features in `Cargo.toml` so the app builds on EL9; don't raise them casually — `adw::AboutWindow` is used instead of `AboutDialog` (1.5) for this reason. Test files (`.cif`, `.vasp`, `POSCAR`, etc.) live at the repo root.

## Architecture

### State model: per-tab document, shared config

The application supports **multiple open structures via tabs**. The state split is critical — most logic bugs come from confusing global vs. per-tab state.

- `AppState` (in `src/state.rs`) holds `tabs: Vec<TabState>`, `active_tab_index`, and the global `Config`.
- `TabState` owns one structure: the loaded `Structure`, `original_structure` (for undo-to-original), `view: ViewState`, `interaction: InteractionState`, per-tab `style: RenderStyle`, plus cached analysis results (`kpath_result`, `void_result`, `bvs_cache`).
- `AppState` is wrapped in `Rc<RefCell<...>>` and threaded through GTK callbacks. **Always go through `state.borrow().active_tab()` / `active_tab_mut()`** — never assume the structure is at a known index. When adding new tab-scoped data, put it on `TabState`, not `AppState`.
- The drawing closure in `ui::create_tab_content` captures a `tab_id` (the tab's index at creation time). Reading via `st.tabs[tid]` requires bounds-checking because tabs can be removed.

### Module layout

Top-level modules each have both a `foo.rs` declaring the module and a `foo/` directory with submodules:

- `io/` — format parsers/writers. `io::load_structure(path)` dispatches by extension and falls back to POSCAR for extensionless files (also handles `POSCAR`/`CONTCAR` filenames). Formats: CIF (and magnetic `.mcif`), OVF magnetization (`.ovf`/`.omf`), XYZ (incl. extended), POSCAR/CONTCAR/`.vasp`, QE input (`.in`/`.pwi`/`.qe`) and output (`.out`/`.log`), VASP `vasprun.xml`, LAMMPS dumps (recognised by content, any extension), SPR-KKR (`.inp`/`.pot`/`.sys`), CHGCAR (volumetric — handled separately via Analysis menu).
- Multi-frame files (vasprun.xml, QE relax/vc-relax/MD output, LAMMPS dump, OVF snapshot series): `io::load_document(path, config.open_frame)` returns the first or last frame (Preferences) as the structure plus a `model::trajectory::Trajectory` of all frames, kept as `TabState::trajectory`. `load_structure` alone gives the last frame for QE/vasprun and the first for LAMMPS. OVF: opening any file of a numbered series (`m000042.ovf`: same name up to a trailing number, same folder and extension) or a multi-segment Spirit file gives a trajectory of every snapshot, showing the opened one; positions are one `Arc` shared by all frames, only `Frame::magmoms` change, and long series are thinned to `ovf::SERIES_BUDGET` cell-moments in all. Files are opened on a worker via `menu::actions_file::open_path`.
- `model/` — pure data types: `Structure { lattice, atoms, formula, is_periodic }`, `Atom`, element table, Miller plane, BVS parameters. `is_periodic` matters: XYZ → false unless a `Lattice=` is present; everything else → true. Operations inherit it.
- `physics/` — split into `analysis/` (read-only computations: Bravais, k-path, voids, voronoi/BZ, XRD, charge density, symmetry) and `operations/` (transforms that mutate structure: supercell, slab, basis, miller, conversion). `bond_valence/` is its own thing with a per-tab cache.
- `rendering/` — Cairo painter. `scene::calculate_scene` projects atoms, `painter::draw_*` rasterizes, `sprite_cache` is an LRU pixel cache for atom sprites (size lives on `RenderStyle`, fresh per session). PDF/PNG/SVG export via `cairo-rs`.
- `ui/` — GTK widgets. `analysis/` holds the read-only Analysis window (symmetry, XRD, band path, voids; charge density is its own window). `structure/` holds the Structure window for tools that change the structure: Supercell, Basis, Atoms (add/move by typed coordinates), Atom Instances, Magnetic (moments, k-vector order) and Slab as notebook tabs. All but Slab share `structure::preview::Preview`, a flat schematic pane in the same style and isometric projection as the Slab canvas (white background, flat dots, wireframe cell; deliberately not the 3D painter); it can show a candidate structure and highlight atoms, and polls a fingerprint so it follows edits made elsewhere. Each tab returns `TabParts` (controls + an `on_enter` refresh hook). `dialogs/` holds the remaining modal dialog (Miller). `interactions.rs` handles mouse/keyboard on the main `DrawingArea`.
- `panels/sidebar.rs` — left sidebar with atom list, style controls, polyhedra, BVS sliders.
- Bonds: `model/bonds.rs` decides which pairs bond — one rule (min, max, on/off) per element pair. Defaults are VESTA's bond list (`model/vesta_bonds.rs`, generated from pymatgen's `vesta_cutoffs.yaml`); elements with no VESTA-listed partner in the structure fall back to covalent radii × `view.bond_cutoff`. `TabState::bond_overrides` stores only the pairs the user edited; `tab.bond_rules()` / `tab.bond_table()` merge them with the defaults for the elements present. Any bond search (painter, polyhedra, CN, trajectory player) must go through `BondTable::bonded`, never a covalent-radius formula of its own. Editor: `ui/bond_rules.rs`.
- Magnetic moments: `Atom::magmom` (Cartesian μB, `None` for most formats), per-frame `Frame::magmoms` for trajectories. VASP moments come from files beside the structure (INCAR `MAGMOM`, OUTCAR), read in `io/magmom.rs`; QE output, mCIF and extended XYZ carry their own. OVF (`io/ovf.rs`, mumax3/OOMMF/Spirit) gives one `Cell` site per mesh cell with m = M/Ms; `TabState::style_ovf_mesh` (run from `style_generic_species`) sets arrows-only and an arrow length of about one cell. The Cairo painter draws them as arrows (`MomentArrow`; the near half is a separate depth-sorted item). An operation that builds new `Atom`s from an existing one should carry `magmom` over, or clear it when the species changes; `commit` logs a warning when an edit drops all moments. Structure → Magnetic Moments (`ui/structure/magnetic_tab.rs`, physics in `physics/operations/magnetic_order.rs`) sets moments and builds k-vector magnetic cells. Writers: `io::magmom::vasp_incar_tags` follows `poscar::write_order` (anything written alongside a POSCAR must); the QE writer splits species per moment.
- Charge Density window (Analysis → Charge Density): a "Slice (2D)" page (Cairo, `charge_density_tab.rs`) and a "3D Isosurface" page (`ui/analysis/charge_density_3d.rs`, GPU). They share `ChargeDensityState`; every control applies immediately (the hidden Update Plot button is re-emitted), and the 3D page follows the state by polling a fingerprint. A bottom bar under the canvas (a homogeneous `Stack` switched with the page) holds the sliders that shape the picture: slice position/hkl offset and isoline count for 2D, histogram/isovalue/opacity for 3D. Set-once options stay in the right pane, which in 3D mode swaps the slice controls for the 3D ones. Isosurfaces come from `physics/analysis/isosurface.rs` (periodic marching tetrahedra, watertight by construction); enclosed charge, "enclose X% of the charge", histograms and field-based ambient occlusion from `physics/analysis/volumetric.rs`. `charge_density_3d/figure_dialog.rs` exports journal figures through `rendering/figure.rs` (PDF/SVG with vector annotations over a lossless raster; PNG/TIFF with dpi in the file); `charge_density_3d/scene_file.rs` saves/loads `*.cview.json` scenes (including channel, crop box and markers) and writes OBJ/PLY meshes. The 3D page can crop to a box around a point (`CropState`: meshes only that `Region`, shows the atom images inside it) and draw `Marker`s (style cube/ring/dot/none; cubes are GL lines, rings, dots and labels are Cairo annotations laid out by `marker_annotations` and `figure::place_labels`, shared by the HUD and exported figures; plain cube labels keep their old figure position so old scenes export unchanged) and highlight atoms (`Highlight`, `HighlightMagnetic` using OUTCAR site moments, resolved by `atom_styles`); with a crop the camera frames `content_points` (atoms, surfaces, markers), not the box. It draws bonds by `model::bonds::default_rules` (it has no tab, so no user overrides) and can show −field (`View::flip`, `negate_field`). Orbital glyphs: `io/ldau.rs` reads DFT+U on-site occupation matrices from an OUTCAR (the one beside the CHGCAR, via `ChargeDensityState::path_a`); `physics/analysis/orbitals.rs` has VASP-ordered real harmonics, occupied orbitals, shell spin density and glyph meshing (isosurfaces on a small local grid); the 3D page draws them in mesh slots 2–3 and rebuilds them on a worker only when `GlyphKey` changes. `CVIEW_CD_FIGURE=out.pdf [CVIEW_CD_SCENE=scene.cview.json] cview CHGCAR` runs a separate instance that opens the CHGCAR on the 3D page, applies the scene, exports the figure and quits (`charge_density_3d::run_figure_script`) — the way to check the 3D page's look without clicking.
- `rendering/gl/` — OpenGL 3.3 core renderer (`glow`, function pointers from GTK's libepoxy) used only by GPU windows, never the main view. Everything goes through the `GpuBackend` trait so the GL code could be swapped for wgpu. Atoms and bonds are instanced impostors; lines are screen-space quads of a set pixel width. With meshes or a section plane it renders offscreen with 4× MSAA and weighted blended OIT; `gl/export.rs` renders publication images in tiles with supersampling and depth-peeled (exact) transparency. Plans: `plan-gpu-rendering.md`, `plan-chgcar-3d.md`.
- `ui/spacegroup_builder.rs` — the Space Group Builder window (File → New from Space Group, Structure → Space Group & Sites). The crystallography lives in `physics/operations/spacegroup_build.rs`: settings and operations from moyo's Hall-symbol table, Wyckoff positions (from `model/wyckoff_table.rs`, a generated copy of moyo's private Wyckoff database — keep it in step with the moyo version, the Hall numbers must match), cell-parameter constraints per setting, site snapping onto special positions, orbit expansion, and `describe()` (structure → group + cell + asymmetric unit) for editing. `lattice_from_params` there is shared with the CIF reader. New structures go through `menu::actions_file::place_structure`, the same path as opening a file.
- `ui/trajectory_player.rs` — Structure → Trajectory Player: plays `TabState::trajectory` on the GPU in its own window, with an energy-per-frame strip (⟨m⟩ per frame when there are no energies). Moments are drawn as GPU arrows (`gl::ArrowInstance`, real shaft-and-cone geometry) styled from the tab's moment settings. "Show in Main View" copies a frame into the tab (undoable). It finds its tab by `Rc::ptr_eq` on the trajectory, not by index. Moment controls (arrows, length, width, colour; on a mesh also arrow thinning and a flat-shaded colour map of every cell) live in the player's own row, not the tab style. `CVIEW_GL_SNAPSHOT=file.png` saves the first frame it draws (debugging the GL path); `CVIEW_PLAYER_SNAPSHOT=file.png cview <multi-frame file>` runs a separate (non-unique) instance that opens the player, saves the whole window and quits — the way to check the player's look without clicking.
- `menu/` — actions split by menu (`actions_file/view/tools/analysis/help`). Each `setup()` registers `gio::SimpleAction`s on the `Application`. Keyboard accels are wired centrally in `menu::build_menu_and_actions`.
- `utils/console.rs` — global singleton for the two bottom-panel TextViews. After `console::init()` is called once in `main.rs`, anywhere can call `console::log_info` / `log_error` / `info_report`. **Don't thread `&TextView` parameters around** — use the console module.
- `utils/logger.rs` — wires the `log` crate to the System Log tab.

### Rendering pipeline

`DrawingArea::set_draw_func` for each tab is the entry point. Order each frame:
1. Pre-pass: if `ColorMode::BondValence`, populate `tab.get_bvs_values()` (mutable borrow, must happen before the immutable read).
2. `rendering::scene::calculate_scene` returns projected atoms + lattice corners + bounds.
3. `rendering::painter::draw_unit_cell` → `draw_structure` → `draw_miller_planes` → `draw_axes` → `draw_selection_box`.

The same scene/painter functions are reused for PDF/PNG export via `rendering::export`.

### Config persistence

`Config` (in `src/config.rs`) is JSON-serialized to the platform config dir (`~/.config/cview/settings.json` on Linux via `directories` crate). `RenderStyle` has manual `Serialize`/`Deserialize` because `atom_cache` and `element_colors` are not persisted (cache is rebuilt; element colors come from `model::elements`). Many fields are tagged `#[serde(default = "...")]` so old config files keep loading after schema changes — **preserve this when adding fields**. Several fields under "LEGACY" are kept solely for backward-compat deserialization and aren't shown in Preferences; don't remove them without a migration.

### Symmetry & cell conventions

Space-group detection and cell standardization use `moyo`. The Setyawan-Curtarolo convention is used for k-path high-symmetry points. `load_conventional` config flag and the Structure menu's "Toggle Primitive/Conventional" switch between the moyo-standardized conventional cell and the input primitive cell.

## CI / Release

Tagged pushes (`v*`, `test-v*`) trigger `.github/workflows/release.yml` which builds Windows (MSYS2/UCRT64), macOS (.app + .dmg with manual `install_name_tool` lib bundling), and Flatpak (GNOME 49 runtime, manifest at `org.mavensgroup.cview.yml`). Flatpak is the only Linux package.

## Conventions in this codebase

- Widgets are downgraded to weak refs (`.downgrade()`) before being captured by long-lived callbacks to avoid reference cycles. Follow this pattern when adding new actions.
- **Every structure edit goes through `TabState::commit(op, new_structure, Edit)`** (`state.rs`), never `tab.structure = Some(..)`. It records the old structure for Undo/Redo (`TabState::history`, 50 steps), clears per-atom state by `Edit` kind (`Species`: same atoms, keep selection and overrides; `Geometry`: indices kept or atoms appended, keep overrides; `Reindexed`: clear both), calls `invalidate_derived()` and `console::structure_changed`. `invalidate_derived()` clears the BVS cache, `kpath_result`, `void_result` and the interstitial overlay together; `invalidate_bvs_cache()` alone leaves stale analysis attached to a structure it no longer describes. Loading a file into a tab is not an edit: it clears `history` instead. After an edit that clears the selection, `AppState::select_atoms(indices)` re-picks atoms by base index.
- The bottom console has two tabs; keep them separate (full policy in the header of `utils/console.rs`). "Structure Info" is the scientific record: structure summaries, BVS reports, measurements, analysis result summaries (`console::info` / `info_report`). "System Log" is what the app did: file I/O and export status, operation events, parser warnings (data dropped or approximated), errors, timings (`log_info` / `log_warn` / `log_error` / `log_debug`; debug is hidden outside debug builds unless `CVIEW_LOG_DEBUG` is set). An operation that replaces the structure writes a log line and a fresh summary through `console::structure_changed`, which `commit` calls for it. Never use `println!`/`eprintln!` for anything the user should see.
- Analysis results (`KPathResult`, `VoidResult`, `InterstitialOverlay`) are cached on the tab so re-opening the Analysis window doesn't recompute. `invalidate_derived()` is what drops them.
- Long analysis runs go through `utils::task::spawn`, which runs the work on a worker thread and delivers the result back on the main loop. Dropping the returned `JobHandle` cancels the job, so a panel keeps one handle and starting a new run supersedes the old one. Work that can take seconds must poll its `CancelToken` and return `None` rather than a partial result.
