# Building Structures

CView provides several tools to manipulate and construct crystal structures. These operations are accessible via the **Tools** menu and enable you to prepare input geometries for *ab-initio* calculations.

---

## Basis Operations

The **Basis** dialog (`Tools → Geometry → Basis/Chemistry`) allows you to perform chemical modifications to your structure.

### Element Substitution

**Global Replacement**: Replace all instances of one element with another throughout the entire structure.

**Use Cases**:
- Alloying studies (e.g., replacing Ni with Co in Ni₂MnGa)
- Doping simulations (e.g., substituting Ca with Sr in perovskites)
- Creating hypothetical structures for screening

**How to use**:
1. Open `Tools → Geometry → Basis/Chemistry`
2. Select the target element from the dropdown
3. Enter the new element symbol
4. Click "Apply Global Substitution"

>[!NOTE]
>This operation preserves all atomic positions and lattice parameters — only the element identity changes.

### Selection-Based Editing

**Selective Modification**: Change the element type of specific atoms rather than all instances.

**Workflow**:
1. Select atoms in the viewport (click + Shift to multi-select)
2. Open the Basis dialog
3. The selected atoms will be highlighted
4. Choose the new element and apply

**Atom Removal**: You can also delete selected atoms to create vacancies or remove unwanted species.

---

## Cell Type Conversion

### Primitive vs. Conventional Cells

Crystallographic structures can be represented in two standard forms:

| Cell Type | Description | Use Case |
|:---|:---|:---|
| **Primitive** | Minimum volume unit containing one formula unit | Electronic structure calculations (smaller = faster) |
| **Conventional** | Standard IUCr representation matching symmetry axes | Visualization, publication figures |

**Example**: Face-centered cubic (FCC) structures:
- **Conventional**: Cubic cell with atoms at corners + face centers (4 atoms)
- **Primitive**: Rhombohedral cell (1 atom)

### Toggling Cell Type

**Keyboard Shortcut**: Press `Ctrl + T` to toggle between primitive and conventional representations.

**Menu Access**: `Tools → Toggle Cell View`

**What happens**:
- CView uses the `moyo` library (spglib wrapper) to detect space group symmetry
- Atomic positions are transformed to the new basis
- The structure is reloaded in the active tab

>[!TIP]
>Use **primitive cells** for DFT calculations to minimize computational cost. Use **conventional cells** for visualizing crystallographic relationships and comparing to literature structures.

### Standardization

When you load a CIF file with arbitrary lattice vectors, CView can standardize the cell to match the IUCr conventions:

1. Detection of space group symmetry
2. Rotation to standard orientation (e.g., c-axis vertical for hexagonal)
3. Choice of conventional or primitive representation

This ensures your structure matches reference databases like ICSD or Materials Project.

---

## Atom Instance Management

The **Atom Instances** dialog controls how periodic images (ghost atoms) are displayed.

**Purpose**: When visualizing a unit cell, atoms near boundaries may have periodic images partially inside the cell. This tool lets you:
- Show/hide ghost atoms outside the central unit cell
- Expand to show neighboring cells (useful for understanding connectivity)
- Clean up cluttered visualizations

**Access**: `Tools → Atom Instances`

>[!NOTE]
>Ghost atoms are always computed for physics calculations (BVS, polyhedra) even when hidden — the "show" setting only affects rendering.

---

## Related Operations

For creating larger structures from unit cells, see:
- [Supercells](supercells.md) — Expand periodicity (NxMxP repetitions)
- [Slab Generation](slabs.md) — Create surfaces with vacuum padding
