# Building Structures

CView provides several tools to manipulate and construct crystal structures. These operations are accessible via the **Structure** menu and enable you to prepare input geometries for *ab-initio* calculations.

Supercell, Basis, Atom Instances and Slab are the four tabs of one **Structure** window. Each menu entry opens the window on its own tab. The first three share a **Preview** pane that shows the structure as a ball-and-stick view, using the same orientation and colours as the main viewport (rotate in the main view and the preview follows):

- **Supercell** previews the structure the current matrix would produce, before you press Transform.
- **Basis** highlights the atoms an action would touch: the atoms selected in the main view, and every atom of the element typed into *Find*.
- **Atom Instances** highlights the rows selected in the list.

The window is not modal, so you can keep it open next to the main view.

---

## Basis Operations

The **Basis** tab (`Structure → Basis...`) allows you to perform chemical modifications to your structure.

### Element Substitution

**Global Replacement**: Replace all instances of one element with another throughout the entire structure.

**Use Cases**:
- Alloying studies (e.g., replacing Ni with Co in Ni₂MnGa)
- Doping simulations (e.g., substituting Ca with Sr in perovskites)
- Creating hypothetical structures for screening

**How to use**:
1. Open `Structure → Basis...`
2. In **Replace element everywhere**, enter the source element in *Find* (its atoms are highlighted in the preview) and the new element in *Replace with*
3. Click **Replace all**

>[!NOTE]
>This operation preserves all atomic positions and lattice parameters — only the element identity changes.

### Selection-Based Editing

**Selective Modification**: Change the element type of specific atoms rather than all instances.

**Workflow**:
1. Select atoms in the viewport (click + Shift to multi-select)
2. Open `Structure → Basis...`; the **Selected atoms** card shows how many atoms are selected, and the preview highlights them
3. Enter the replacement element symbol
4. Click **Change element**

**Atom Removal**: Press `Delete` after selecting atoms to create vacancies or remove unwanted species.

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

**Menu Access**: `Structure → Toggle Primitive/Conventional`

**What happens**:
- CView uses the `moyo` library (spglib wrapper) to detect space group symmetry
- Atomic positions are transformed to the new basis
- The converted cell replaces the active structure. Conversion is derived from the originally loaded structure, so edits made after loading are not carried through this operation.

>[!TIP]
>Use **primitive cells** for DFT calculations to minimize computational cost. Use **conventional cells** for visualizing crystallographic relationships and comparing to literature structures.

### Standardization

The **Positions** card of the Basis tab provides **Standardize positions [0, 1)**, which wraps atomic fractional coordinates into the unit cell. It does not perform crystallographic IUCr standardization; use primitive/conventional conversion for Moyo-based standard cells.

---

## Atom Instance Management

The **Atom Instances** tab provides session-only cosmetic overrides for individual atoms. It lets you filter the atom list and apply or reset a display label and colour for selected sites.

**Access**: `Structure → Atom Instances...`

>[!NOTE]
>These overrides are not written to structure files. Use `View → Hide Symmetric Basis` or `Ctrl + Shift + B` to toggle visible periodic images.

---

## Related Operations

For creating larger structures from unit cells, see:
- [Supercells](supercells.md) — Expand periodicity (NxMxP repetitions)
- [Slab Generation](slabs.md) — Create surfaces with vacuum padding
