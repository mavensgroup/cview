# Plan: Optimization / MD trajectory animation in CView

## Context
CView loads one frame per file. Users want to play back relaxations and MD runs (VASP, QE, LAMMPS). The question was ASE (Python) vs. our own code.

**Recommendation: hybrid, native-first.** Write native Rust readers for the common formats and use ASE only as an optional fallback for the rest. Reasons:
- CView is a self-contained Rust/GTK app that ships as Windows, macOS, Flatpak, DEB and RPM builds. A hard Python/ASE dependency would break all of those, and would break the Flatpak sandbox in particular.
- The formats that matter are simple text and already half-parsed: QE `.out` (the parser in `src/io/qe.rs` already walks every `ATOMIC_POSITIONS` block and discards all but the last), XDATCAR, LAMMPS dump, multi-frame XYZ.
- Spawning Python per file is slow and awkward for 10^3-frame trajectories, and playback itself needs everything in memory in Rust anyway.
- ASE is still worth having for the hard formats (vasprun.xml, OUTCAR, LAMMPS binary/custom dumps). `src/io/sprkkr.rs:880-1000` already has an optional-Python mechanism: an embedded script run with `$CVIEW_PYTHON`, `python3` or `python`, JSON on stdin and stdout, and a clean fallback if the import fails. It can be reused as is.

## Steps

### 1. Data model (`src/model/`, `src/state.rs`)
- Add `Trajectory { frames: Vec<Frame>, idx: usize, playing: bool, fps: f32 }`, with `Frame { structure: Structure, energy: Option<f64>, max_force: Option<f64> }`.
- Add `trajectory: Option<Trajectory>` to `TabState` (per-tab, not `AppState`). `view` is already per tab, so the camera stays put between frames.
- Keep frames as full `Structure`s for simplicity. Delta-compress later only if memory becomes a problem.

### 2. Native readers (`src/io/`)
- Add `io::load_trajectory(path) -> io::Result<Vec<Frame>>` next to `load_structure` (`src/io.rs:24`). Single-frame files return one frame, so the GUI can use one entry point later.
- `qe.rs`: change `parse_output` (line 28) to push a frame at the end of each `ATOMIC_POSITIONS` block, carrying the current lattice, instead of `atoms.clear()` at line ~64. Also parse `!    total energy` so we get an energy per frame. `load_structure` takes the last frame, so existing behaviour is unchanged. Fixed-cell relax needs the initial cell from the `crystal axes` block (today it errors with "No CELL_PARAMETERS found").
- New `xdatcar.rs`: header plus repeated `Direct configuration=` blocks. Variable-cell XDATCAR repeats the header per frame.
- New `lammps_dump.rs`: `ITEM: TIMESTEP / NUMBER OF ATOMS / BOX BOUNDS / ATOMS`, including triclinic boxes. Map type ids to elements via a user-supplied or guessed mapping.
- `xyz.rs`: add a multi-frame loop (the first-frame-only behaviour at lines 51-54 stays for `load_structure`). Reuse the extended-XYZ `Lattice=` parsing.
- Optional ASE fallback, `src/io/ase_traj.rs` plus an embedded `.py` helper, modeled on the sprkkr helper. It is used for `vasprun.xml`, `OUTCAR` and anything else the native readers reject. Exit code 3 means ASE is missing, and the failure is logged to the System Log.

### 3. Loading (`src/menu/actions_file.rs:~149-180`, `src/lib.rs:228`)
- If `load_trajectory` returns more than one frame, install it on the tab: set `structure` to frame 0 and `original_structure` to frame 0.
- Parse on a worker through `utils::task::spawn` (`src/utils/task.rs:76`) with `CancelToken` polling. Keep the `JobHandle` on the tab or window. Large trajectories should not freeze the UI.
- Log through `console::log_info` ("loaded N frames"). Put a scientific summary (initial and final energy, max force) in Structure Info via `console::info`.

### 4. Playback
- New `src/menu/actions_trajectory.rs`, registered in `src/menu.rs:17` with the other `setup()` calls: play/pause (`SimpleAction::new_stateful`), step forward, step back, first, last. Add accelerators centrally in `menu::build_menu_and_actions`.
- Timer: `glib::timeout_add_local(1000/fps)`. Each tick advances `idx`, calls `set_frame(tab, idx)` and `da.queue_draw()`. Hold the source id so pause and tab close can cancel it. Capture weak refs per the codebase convention.
- `set_frame`: replace `tab.structure`, clear `interaction.selected`, and call `invalidate_derived()` only when needed (BVS colouring, or when the user stops playing). Skip `overrides.clear()` while the atom count is constant.
- Control bar: a small `gtk::Box` under the drawing area with a play/pause button, a `Scale` for the frame index, an fps spin button and an energy/frame label. It is created in `create_tab_content` (`src/ui.rs:32`) and only shown when `trajectory.is_some()`.
- Guard against tab removal: re-check the tab index each tick, as the draw closure already must.

### 5. Optional later extras (not in the first cut)
- Energy and force vs. frame plot (plotters is already a dependency) with a click-to-seek cursor.
- Export of frames to GIF/PNG sequence via the existing `rendering::export`.
- Per-frame force arrows.

## Critical files
`src/io.rs`, `src/io/qe.rs`, `src/io/xyz.rs`, `src/io/sprkkr.rs` (pattern to copy), `src/state.rs` (`TabState`, `invalidate_derived`), `src/ui.rs` (`create_tab_content`), `src/menu.rs`, `src/menu/actions_file.rs`, `src/utils/task.rs`, new `src/io/{xdatcar,lammps_dump}.rs` and `src/menu/actions_trajectory.rs`.

## Performance note
`calculate_scene` rebuilds all projected atoms, ghost atoms and cell corners on every draw (`src/rendering/scene.rs:60`), with no caching. It is fine for 10-30 fps on small and medium cells, since rotation drag already redraws this way. BVS colouring and large supercells are the slow cases. If playback stutters, cache the scene per frame or lower the default fps.

## Verification
- Unit tests for each reader on small fixtures placed at the repo root: a QE vc-relax `.out` (frame count, last frame equals the old `load_structure` result), an XDATCAR, a LAMMPS dump, and a 3-frame XYZ.
- `cargo check` and `cargo clippy`.
- `cargo run --release -- relax.out`: confirm the frame slider appears, play/pause/step work, rotating the view during playback keeps the camera, closing the tab mid-playback does not panic, and a single-frame file shows no controls.
- Test the ASE fallback both with ASE installed and with it absent (`CVIEW_PYTHON=/nonexistent`). Absent must give a clear System Log message and no crash.
