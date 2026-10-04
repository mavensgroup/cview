# Loading & Visualization

This section outlines the core workflows for importing structure files, managing the workspace, and controlling the visual representation of crystal structures.


## File Operations

### Opening Files
To load a structure, navigate to `File → Open (Ctrl + O)` in the application menu. CView utilizes the native file chooser of your operating system to ensure a familiar experience.

**Supported Formats:**
CView automatically detects the file type based on extension and content. You can open the following formats:

| Format | Extensions | Description |
| :--- | :--- | :--- |
| **CIF** | `.cif` | Standard Crystallographic Information Files. |
| **VASP** | `POSCAR`, `CONTCAR`, `.vasp` | Standard VASP structure inputs and outputs. |
| **VASP run** | `vasprun.xml` (any name; recognised by content) | Every ionic step of a relaxation or MD run, with its energy and largest force. |
| **Quantum Espresso** | `.in`, `.out`, `.pwi`, `.qe` | Inputs, and every geometry of a relax, vc-relax or MD output (starting with the input structure), with energies and forces. |
| **LAMMPS** | `.dump`, `.lammpstrj`, `.dat`, any name starting with `ITEM: TIMESTEP` | Text dumps: wrapped, scaled or unwrapped positions; orthogonal or triclinic boxes; every frame. Species come from an `element` or `mass` column, otherwise atoms are shown as `Type n`. |
| **SPR-KKR** | `.pot`, `.pot_new`, `.sys` | Munich SPR-KKR potential and system files. Chemical disorder (CPA sites with several occupants) is read as mixed sites and written back as CPA sites. Reading needs nothing extra (`.pot`, `.pot_new`, `.pot_out`). Writing uses CView's own writer by default; set the environment variable `CVIEW_PYTHON` to a Python that has ase2sprkkr to write with ase2sprkkr instead — an optional improvement, never a requirement (see [Building Structures](building.md#exporting-a-disordered-alloy-to-spr-kkr)). |
| **XYZ** | `.xyz` | Cartesian coordinates (Standard and Extended XYZ). |
| **PDB** | `.pdb`, `.ent` | Protein Data Bank records. Reads the `CRYST1` cell, `ATOM`/`HETATM` sites, occupancies and formal charges; the first `MODEL` only. Files without a real cell load as non-periodic molecules. |

>[!NOTE]
>**Why LAMMPS, in a DFT-oriented tool?** A LAMMPS dump is, at its core, the same data CView already parses for every other trajectory: a box plus a per-atom coordinate list, one frame after another — structurally the same problem as an Extended XYZ trajectory or a QE `vc-relax` log, just with LAMMPS's own column layout. Reading it slots into the trajectory machinery built for `vasprun.xml`/QE/MD output rather than opening a new area of the code. CView does not run LAMMPS, does not know about force fields, and writes no LAMMPS input — it is a geometry viewer for a dump file someone already produced, exactly as it is for a VASP or QE run.

### Trajectories (Relaxation and MD)

Files with several frames — `vasprun.xml`, Quantum Espresso relax/MD output, LAMMPS dumps — open as one structure in the main view: the **first** or **last** frame, set in `Preferences → General → Relaxation / MD files open at`. All frames are kept.

`Structure → Trajectory Player… (Ctrl + Shift + P)` plays them in a separate window drawn on the GPU:

* Play, step and jump with the buttons or **Space**, **, / .** (or **← / →**), **Home / End**; set the speed in frames per second.
* For relaxations, the energy of every step is plotted under the view (click to jump), with the energy, ΔE and largest force of the current step on screen.
* **Show in Main View** copies the current frame into the main window for measurements, analysis and vector export; **Undo** restores the previous structure.
* **Save Image…** writes the 3D view as a PNG.

### Tab Management
CView uses a tabbed interface to handle multiple structures simultaneously. The application employs a **smart loading strategy** to keep the workspace clean:

* **Replacement Mode:** If the current active tab is empty (labelled "Untitled" with no structure), opening a file will replace this tab.
* **New Tab Mode:** If a structure is already loaded, the new file will open in a separate, closable tab.

>[!NOTE]
>Upon successfully loading a file, the application logs the event in the **Interaction Log** and automatically refreshes the **Sidebar** to display the atom list for the new structure.

### Saving & Exporting

#### Saving Data
To convert a loaded structure into a different format, use `File → Save As (Shift+ Ctrl+ S)`. The output format is determined by the selected file filter in the dialog.

##### Available Output Formats:

- CIF (*.cif)
- VASP POSCAR (POSCAR, *.vasp)
- SPR-KKR Potential (*.pot)
- Quantum Espresso Input (*.in)
- XYZ (*.xyz)
- PDB (*.pdb)


## Visualization Controls
Once a structure is loaded, the View menu provides tools to orient and inspect the crystal lattice.

### Standard Views
Quickly align the camera to specific crystallographic axes to inspect symmetry or stacking sequences.

- View Along A: Aligns camera with the a-axis (y=−90°).
- View Along B: Aligns camera with the b-axis (x=90°).
- View Along C: Aligns camera with the c-axis (Standard Plan View).
- Reset View: Restores the default zoom (1.0) and rotation (0,0).

### Rotation Modes
You can customize the pivot point around which the camera rotates, depending on whether you are inspecting the atomic cluster or the lattice boundaries.

|Mode|Description|
|:---|:---|
|Centroid|	Rotates around the geometric centre of the atoms. Best for inspecting molecules or specific bonding environments.|
|Unit Cell|	Rotates around the centre of the unit cell box. Best for understanding the lattice boundaries relative to the origin.|

### Exporting Visuals
For publications and presentations, `CView` offers high-fidelity export options via the `File → Export (Ctrl + E)` menu.

PNG Image: Renders a high-resolution raster image (default resolution: $2000\times 1500$ pixels). Ideal for slides and quick sharing.

PDF Document: Exports the scene as a vector graphic. This is recommended for academic papers, as it allows for infinite scaling without loss of quality.

SVG Document: Exports an editable vector graphic.

---

## Advanced Appearance Controls

### Sidebar Overview

The **Sidebar** (right panel) is the primary control interface for customizing the visual representation. It contains several collapsible sections:

1. **View Controls**: Camera orientation, rotation center
2. **Appearance**: Atom size, bond thickness, colors
3. **Bond Valence**: BVS-based coloring (see [BVS Guide](bvs.md))
4. **Element Colors**: Per-element colour, transparency, and polyhedra controls

### Color Modes

CView offers two coloring schemes, accessible via the **Bond Valence** expander's **Color Mode** dropdown:

| Mode | Description | Use Case |
|:---|:---|:---|
| **Element Colors** | Standard CPK colours (C=gray, O=red, etc.) | General visualization, publication figures |
| **Bond Valence** | Heatmap based on the magnitude of BVS deviation | Identifying poorly matched coordination environments |

**Bond Valence Mode**: Colors atoms by the absolute difference between calculated and expected BVS:
- **Green**: Good agreement
- **Yellow/orange**: Increasing deviation
- **Red**: Large deviation

Selecting this mode recalculates BVS and writes a detailed report to the Structure Info panel.

See the [Bond Valence Sum Guide](bvs.md) for details on the calculation.

### Transparency Controls

Each element in the structure has an individual transparency slider in the **Atom List** section:

- **Use case**: Highlight specific atomic species by making others semi-transparent
- **Example**: In a LiCoO₂ battery cathode, make Li transparent to see the CoO₂ layers clearly

**Tip**: Combine transparency with polyhedra (see below) to visualize coordination environments.

### Bond Customization

The **Bonds** section in the sidebar controls:

- **Bond Radius**: Thickness of bond cylinders (default: 0.12 Å)
- **Bond Tolerance**: Multiplier on the covalent-radius sum used for drawing bonds (default: 1.15)
- **Bond Color**: RGB picker for custom bond colors

Bonds are drawn between atoms whose distance $d$ satisfies:
$$
d \leq \text{cutoff} \times (r_{\text{cov},A} + r_{\text{cov},B})
$$
where $r_{\text{cov}}$ are the covalent radii from the internal database.

---

## Coordination Polyhedra

CView can visualize **coordination polyhedra** around cation centers — a feature essential for understanding ionic and metal-oxide structures.

### What are Coordination Polyhedra?

A coordination polyhedron is the 3D shape formed by connecting the nearest-neighbor anions around a central cation. Common geometries include:

- **Tetrahedral** (CN = 4): SiO₄ in silicates
- **Octahedral** (CN = 6): TiO₆ in perovskites  
- **Cubic** (CN = 8): CsCl structure

### Accessing Polyhedra Controls

**Location**: Sidebar → **Appearance** → **Element Colors**

**Controls**:
1. **Auto-detect Polyhedra** button: Automatically enables polyhedra for elements with average coordination number 4–8
2. **Per-element checkboxes**: Manually enable/disable polyhedra for specific elements
   - Checkbox labels show the average CN (e.g., "Ti (CN 6)")
3. **Poly Opacity** slider: Adjust polyhedra opacity (5–95%; higher is more opaque). This sets the value for the current structure; the default for new tabs is in Preferences → Polyhedra.
4. **Color by** dropdown: what colours the polyhedra
   - **Element** (the central atom's colour) or **Custom color**
   - **A computed property**: coordination number, mean bond length, Baur distortion Δ, quadratic elongation ⟨λ⟩, bond-angle variance σ², or volume. A colour scale is drawn in the bottom-right of the viewport and in exported images. The scale follows the range of the polyhedra on screen; if they all share one value, the legend states it instead of drawing a gradient. Where a property is undefined for a polyhedron (⟨λ⟩ and σ² need a reference shape for that coordination number), it is drawn grey.
5. **Polyhedra report** button: writes a summary to *Structure Info*: per polyhedron type, the count, mean metrics, and how the polyhedra link (the mean number of other polyhedra each shares one vertex = corner, two = edge, three or more = face). Linkage is computed from the structure's own periodic images, so polyhedra on the cell boundary are counted correctly.

**Inspecting one polyhedron**: click the central atom of a polyhedron in the viewport. Besides the usual selection report, *Structure Info* lists its coordination number, mean and range of bond lengths, Δ, ⟨λ⟩, σ² and volume.

### Appearance (Preferences → Polyhedra)

How polyhedra are drawn is a saved preference, separate from what is shown:

- **Edges**: None, Subtle (default) or Strong. Only real polyhedron edges are drawn, not the diagonals across flat faces.
- **Default opacity**, **Back-face opacity** (faces turned away from you are drawn fainter, which keeps overlapping polyhedra readable), **Shading strength** (0 is flat), **Mute element colors**, and the **Colormap** used for property colouring.

Changes apply to the current structure immediately and are used for new tabs. Which elements are shown, the colour-by choice and the bond range belong to the structure and are not saved.

### How Polyhedra are Computed

**Algorithm** (implemented in `rendering/polyhedra.rs`):
1. Identify cation centers (user-selected or auto-detected)
2. Find nearest anion neighbors within the bond cutoff distance
3. Compute the **convex hull** of the anion positions
4. Merge coplanar vertices into single flat faces (a square face is one polygon, triangulated once)
5. Render with proper depth sorting (Polyhedra → Bonds → Atoms)

**Critical detail**: Polyhedra vertices are restricted to **anions only**. This prevents chemically meaningless polyhedra (e.g., around O in oxides).

>[!NOTE]
>The two-tier ghost system ensures polyhedra are correctly computed across periodic boundaries — ghost atoms used for coordination detection are different from those rendered visually.

### Settings

Polyhedra use the configured bond tolerance, a maximum bond-distance cap, and an internal coordination range of 4–12. The sidebar exposes element selection, auto-detection, opacity, colour-by and the bond range; Preferences exposes how polyhedra are drawn. Coordination-number filters are not exposed.

### Practical Example: BaTiO₃

In the cubic perovskite BaTiO₃:
- **Ti⁴⁺** is octahedrally coordinated by 6 oxygen atoms → **TiO₆ octahedra**
- **Ba²⁺** has 12-fold coordination → **Cuboctahedral** (can be hidden if noisy)

**Workflow**:
1. Load BaTiO₃ structure
2. Click "Auto-Detect" in the sidebar
3. Ti polyhedra appear as blue octahedra
4. Adjust transparency to see through to the unit cell

This visualization immediately reveals the corner-sharing connectivity characteristic of perovskites.

---

## Keyboard Shortcuts

| Shortcut | Action |
|:---|:---|
| `Ctrl + O` | Open file |
| `Shift + Ctrl + S` | Save structure as... |
| `Ctrl + E` | Export image (PNG/PDF/SVG) |
| `Ctrl + ,` | Preferences |
| `Ctrl + W` | Close tab |
| `Ctrl + Z` | Undo (e.g. atom deletion) |
| `T` | Toggle Primitive ↔ Conventional cell (canvas focused) |
| `Ctrl + Shift + C` | Matrix transformation tool |
| `Ctrl + M` | Add Miller plane |
| `Ctrl + B` | Toggle bonds |
| `Ctrl + Shift + B` | Toggle full-unit-cell/ghost display |
| `Ctrl + Shift + P` | Trajectory Player (relaxation / MD files) |
| **Mouse scroll** | Zoom in/out |
| **Left-click drag** | Rotate structure |
| **Shift + click** | Multi-select atoms |

---

## Performance Notes

The **main viewer** — structure loading, all analysis, Supercell/Basis/Slab, and PNG/PDF/SVG export — is CPU-based rendering using GTK4/Cairo:
- Smooth interaction up to ~5000 atoms
- Real-time rotation and zoom
- No GPU drivers required (runs on any laptop)

Two **optional** windows use OpenGL 3.3 instead, because Cairo cannot do what they need: the [3D Isosurface](charge_density.md#3d-isosurfaces) page has to depth-sort and alpha-blend overlapping translucent lobes in real time as you drag a slider, and the [Trajectory Player](#trajectories-relaxation-and-md) has to redraw every atom at interactive frame rates while scrubbing through hundreds of MD/relaxation steps — both are workloads Cairo's CPU rasterizer is not built for. Neither window is required for normal use, and the GPU floor is low (OpenGL 3.3, any laptop from the last decade). If you never open them, CView never touches your GPU.

For nanoparticles or proteins with tens of thousands of atoms in the main view, consider specialized GPU-accelerated tools like OVITO or VMD.
