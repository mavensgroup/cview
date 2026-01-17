# Loading & Visualization

This section outlines the core workflows for importing structure files, managing the workspace, and controlling the visual representation of crystal structures.

---

## File Operations

### Opening Files
To load a structure, navigate to **File → Open** in the application menu. CView utilizes the native file chooser of your operating system to ensure a familiar experience.

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

* **Replacement Mode:** If the current active tab is empty (labeled "Untitled" with no structure), opening a file will replace this tab.
* **New Tab Mode:** If a structure is already loaded, the new file will open in a separate, closable tab.

```admonish note title="Workspace Feedback"
Upon successfully loading a file, the application logs the event in the **Interaction Log** and automatically refreshes the **Sidebar** to display the atom list for the new structure.
```

### Saving & Exporting

#### Saving Data
To convert a loaded structure into a different format, use File → Save As. The output format is determined by the selected file filter in the dialog.

##### Available Output Formats:

- CIF (*.cif)
- VASP POSCAR (POSCAR, *.vasp)
- SPR-KKR Potential (*.pot)
- Quantum Espresso Input (*.in)
- XYZ (*.xyz)

### Exporting Visuals
For publications and presentations, CView offers high-fidelity export options via the File → Export menu.

PNG Image: Renders a high-resolution raster image (default resolution: 2000x1500 pixels). Ideal for slides and quick sharing.

PDF Document: Exports the scene as a vector graphic. This is recommended for academic papers, as it allows for infinite scaling without loss of quality.

#### Visualization Controls
Once a structure is loaded, the View menu provides tools to orient and inspect the crystal lattice.

##### Standard Views
Quickly align the camera to specific crystallographic axes to inspect symmetry or stacking sequences.

- View Along A: Aligns camera with the a-axis (y=−90°).
- View Along B: Aligns camera with the b-axis (x=90°).
- View Along C: Aligns camera with the c-axis (Standard Plan View).
- Reset View: Restores the default zoom (1.0) and rotation (0,0).

#### Rotation Modes
You can customize the pivot point around which the camera rotates, depending on whether you are inspecting the atomic cluster or the lattice boundaries.

|Mode|Description|
|:---|:---|
|Centroid|	Rotates around the geometric center of the atoms. Best for inspecting molecules or specific bonding environments.|
|Unit Cell|	Rotates around the center of the unit cell box. Best for understanding the lattice boundaries relative to the origin.|
