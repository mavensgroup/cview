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
| **Quantum Espresso** | `.in`, `.out`, `.pwi`, `.qe` | Reads atomic positions and cell parameters from input/output logs. |
| **SPR-KKR** | `.pot`, `.sys` | Munich SPR-KKR potential and system files. |
| **XYZ** | `.xyz` | Cartesian coordinates (Standard and Extended XYZ). |

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

---

## Advanced Appearance Controls

### Sidebar Overview

The **Sidebar** (right panel) is the primary control interface for customizing the visual representation. It contains several collapsible sections:

1. **View Controls**: Camera orientation, rotation center
2. **Appearance**: Atom size, bond thickness, colors
3. **Bond Valence**: BVS-based coloring (see [BVS Guide](bvs.md))
4. **Atom List**: Per-element visibility and transparency

### Color Modes

CView offers three distinct coloring schemes, accessible via the **Color Mode** dropdown in the sidebar:

| Mode | Description | Use Case |
|:---|:---|:---|
| **Element** | Standard CPK colors (C=gray, O=red, etc.) | General visualization, publication figures |
| **Bond Valence** | Heatmap based on oxidation state deviation | Identifying strained bonds, charge transfer |
| **Uniform** | Single color for all atoms | Minimalist renders, presentations |

**Bond Valence Mode**: Colors atoms by their BVS deviation:
- **Blue**: Under-coordinated (BVS < expected valence)
- **White**: Ideal coordination (BVS ≈ expected valence)
- **Red**: Over-coordinated (BVS > expected valence)

See the [Bond Valence Sum Guide](bvs.md) for details on the calculation.

### Transparency Controls

Each element in the structure has an individual transparency slider in the **Atom List** section:

- **Use case**: Highlight specific atomic species by making others semi-transparent
- **Example**: In a LiCoO₂ battery cathode, make Li transparent to see the CoO₂ layers clearly

**Tip**: Combine transparency with polyhedra (see below) to visualize coordination environments.

### Bond Customization

The **Bonds** section in the sidebar controls:

- **Bond Radius**: Thickness of bond cylinders (default: 0.1 Å)
- **Bond Cutoff**: Maximum distance for drawing bonds (default: 2.5× covalent radius sum)
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

**Location**: Sidebar → **Appearance** section → Polyhedra expander

**Controls**:
1. **Auto-Detect** button: Automatically enables polyhedra for elements with average coordination number 4–8
2. **Per-element checkboxes**: Manually enable/disable polyhedra for specific elements
   - Checkbox labels show the average CN (e.g., "Ti (CN 6)")
3. **Transparency slider**: Adjust polyhedra opacity (0–100%)

### How Polyhedra are Computed

**Algorithm** (implemented in `rendering/polyhedra.rs`):
1. Identify cation centers (user-selected or auto-detected)
2. Find nearest anion neighbors within the bond cutoff distance
3. Compute the **convex hull** of the anion positions
4. Filter degenerate faces (coplanar vertices)
5. Render with proper depth sorting (Polyhedra → Bonds → Atoms)

**Critical detail**: Polyhedra vertices are restricted to **anions only**. This prevents chemically meaningless polyhedra (e.g., around O in oxides).

>[!NOTE]
>The two-tier ghost system ensures polyhedra are correctly computed across periodic boundaries — ghost atoms used for coordination detection are different from those rendered visually.

### Settings

**Coordination Number Filter** (in `Preferences`):
- **Min CN**: Minimum coordination to display (default: 4)
- **Max CN**: Maximum coordination to display (default: 12)
- Filters out isolated atoms or highly over-coordinated sites

**Color Mode**:
- **Element**: Polyhedra inherit the color of the central cation
- **Coordination**: Color-coded by CN (e.g., CN=4 → yellow, CN=6 → blue)

**Max Bond Distance**: Hard cutoff in Ångströms for coordination bonds (overrides covalent radius heuristic)

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
| `Ctrl + S` | Save structure |
| `Shift + Ctrl + S` | Save structure as... |
| `Ctrl + E` | Export image (PNG/PDF) |
| `Ctrl + T` | Toggle Primitive ↔ Conventional cell |
| **Mouse scroll** | Zoom in/out |
| **Left-click drag** | Rotate structure |
| **Shift + click** | Multi-select atoms |

---

## Performance Notes

CView is optimized for **CPU-based rendering** using GTK4/Cairo:
- Smooth interaction up to ~5000 atoms
- Real-time rotation and zoom
- No GPU drivers required (runs on any laptop)

For larger systems (e.g., nanoparticles, proteins), consider specialized GPU-accelerated tools like OVITO.
