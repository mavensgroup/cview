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

System dependencies (Fedora): `sudo dnf install gcc gtk4-devel libadwaita-devel`. Test files (`.cif`, `.vasp`, `POSCAR`, etc.) live at the repo root.

## Architecture

### State model: per-tab document, shared config

The application supports **multiple open structures via tabs**. The state split is critical — most logic bugs come from confusing global vs. per-tab state.

- `AppState` (in `src/state.rs`) holds `tabs: Vec<TabState>`, `active_tab_index`, and the global `Config`.
- `TabState` owns one structure: the loaded `Structure`, `original_structure` (for undo-to-original), `view: ViewState`, `interaction: InteractionState`, per-tab `style: RenderStyle`, plus cached analysis results (`kpath_result`, `void_result`, `bvs_cache`).
- `AppState` is wrapped in `Rc<RefCell<...>>` and threaded through GTK callbacks. **Always go through `state.borrow().active_tab()` / `active_tab_mut()`** — never assume the structure is at a known index. When adding new tab-scoped data, put it on `TabState`, not `AppState`.
- The drawing closure in `ui::create_tab_content` captures a `tab_id` (the tab's index at creation time). Reading via `st.tabs[tid]` requires bounds-checking because tabs can be removed.

### Module layout

Top-level modules each have both a `foo.rs` declaring the module and a `foo/` directory with submodules:

- `io/` — format parsers/writers. `io::load_structure(path)` dispatches by extension and falls back to POSCAR for extensionless files (also handles `POSCAR`/`CONTCAR` filenames). Formats: CIF, XYZ (incl. extended), POSCAR/CONTCAR/`.vasp`, QE input (`.in`/`.pwi`/`.qe`) and output (`.out`/`.log`, extracts final frame from vc-relax), SPR-KKR (`.inp`/`.pot`/`.sys`), CHGCAR (volumetric — handled separately via Analysis menu).
- `model/` — pure data types: `Structure { lattice, atoms, formula, is_periodic }`, `Atom`, element table, Miller plane, BVS parameters. `is_periodic` matters: XYZ → false unless a `Lattice=` is present; everything else → true. Operations inherit it.
- `physics/` — split into `analysis/` (read-only computations: Bravais, k-path, voids, voronoi/BZ, XRD, charge density, symmetry) and `operations/` (transforms that mutate structure: supercell, slab, basis, miller, conversion). `bond_valence/` is its own thing with a per-tab cache.
- `rendering/` — Cairo painter. `scene::calculate_scene` projects atoms, `painter::draw_*` rasterizes, `sprite_cache` is an LRU pixel cache for atom sprites (size lives on `RenderStyle`, fresh per session). PDF/PNG/SVG export via `cairo-rs`.
- `ui/` — GTK widgets. `analysis/` holds the read-only Analysis window (symmetry, XRD, band path, voids; charge density is its own window). `structure/` holds the Structure window for tools that change the structure: Supercell, Basis, Atom Instances and Slab as notebook tabs. The first three share `structure::preview::Preview`, a ball-and-stick pane drawn with the real painter from a scratch `TabState`; it mirrors the main view's orientation, can show a candidate structure and highlight atoms, and polls a fingerprint so it follows edits made elsewhere. Each tab returns `TabParts` (controls + an `on_enter` refresh hook). `dialogs/` holds the remaining modal dialog (Miller). `interactions.rs` handles mouse/keyboard on the main `DrawingArea`.
- `panels/sidebar.rs` — left sidebar with atom list, style controls, polyhedra, BVS sliders.
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

Tagged pushes (`v*`, `test-v*`) trigger `.github/workflows/release.yml` which builds Windows (MSYS2/UCRT64), macOS (.app + .dmg with manual `install_name_tool` lib bundling), Flatpak (GNOME 49 runtime, manifest at `org.mavensgroup.cview.yml`), and Linux DEB+RPM (`cargo-deb`, `cargo-generate-rpm`). The Cargo.toml contains the packaging metadata for all three Linux paths.

## Conventions in this codebase

- Widgets are downgraded to weak refs (`.downgrade()`) before being captured by long-lived callbacks to avoid reference cycles. Follow this pattern when adding new actions.
- Derived state is invalidated explicitly via `tab.invalidate_derived()` after any structure mutation (delete, undo, supercell, cell conversion, element substitution). It clears the BVS cache, `kpath_result`, `void_result` and the interstitial overlay together. New mutations must call it; `invalidate_bvs_cache()` alone is not enough and leaves stale analysis attached to a structure it no longer describes.
- The bottom console has two tabs; keep them separate (full policy in the header of `utils/console.rs`). "Structure Info" is the scientific record: structure summaries, BVS reports, measurements, analysis result summaries (`console::info` / `info_report`). "System Log" is what the app did: file I/O and export status, operation events, parser warnings (data dropped or approximated), errors, timings (`log_info` / `log_warn` / `log_error` / `log_debug`; debug is hidden outside debug builds unless `CVIEW_LOG_DEBUG` is set). An operation that replaces the structure (supercell, slab, cell conversion, substitution) calls `console::structure_changed(op, &structure)`, which writes a log line and a fresh summary. Never use `println!`/`eprintln!` for anything the user should see.
- Analysis results (`KPathResult`, `VoidResult`, `InterstitialOverlay`) are cached on the tab so re-opening the Analysis window doesn't recompute. `invalidate_derived()` is what drops them.
- Long analysis runs go through `utils::task::spawn`, which runs the work on a worker thread and delivers the result back on the main loop. Dropping the returned `JobHandle` cancels the job, so a panel keeps one handle and starting a new run supersedes the old one. Work that can take seconds must poll its `CancelToken` and return `None` rather than a partial result.
