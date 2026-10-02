# Building Structures

CView provides several tools to manipulate and construct crystal structures. These operations are accessible via the **Structure** menu and enable you to prepare input geometries for *ab-initio* calculations.

Supercell, Basis, Atom Instances and Slab are the four tabs of one **Structure** window. Each menu entry opens the window on its own tab. The first three share a **Preview** pane: a flat schematic of the cell in the same style as the Slab cutting-plane view (white canvas, element-coloured dots, wireframe cell, the same fixed isometric orientation). It is a preview, not the 3D viewport, so it has no shading or bonds. Highlighted atoms get a dark ring:

- **Supercell** previews the structure the current matrix would produce, before you press Transform.
- **Basis** highlights the atoms an action would touch: the atoms selected in the main view, and every atom of the element typed into *Find*.
- **Atom Instances** highlights the rows selected in the list.

The window is not modal, so you can keep it open next to the main view.

---

## Basis Operations

The **Basis** tab (`Structure → Basis...`) allows you to perform chemical modifications to your structure.

### Element Substitution

**Replace an element**: replace all instances of one element with another, or only some of them.

**Use Cases**:
- Alloying studies (e.g., replacing Ni with Co in Ni₂MnGa)
- Doping simulations (e.g., substituting Ca with Sr in perovskites)
- **Partial substitution and disordered models** (e.g., one O of the three in BaTiO₃ replaced by N, or a random 25% of the O sites in a supercell)
- Creating hypothetical structures for screening

**How to use**:
1. Open `Structure → Basis...`
2. In **Replace an element**, enter the source element in *Find* and the new element in *Replace with*. The *Atoms to replace* count starts at all of them ("of 3" in BaTiO₃).
3. To replace only some, lower the count. The atoms that will change are ringed in the preview.
   - By default these are the first atoms of that element, in the order of the atom list.
   - Tick **Choose at random** to draw them at random instead, and **Pick again** for another draw. A random draw is reproducible within a session (it is not drawn from the system clock), so repeating a step gives the same atoms.
4. Click **Replace**. Replacing fewer than all is logged as a partial substitution (for example "1 of 3 O → N").

For a disordered model you usually want a supercell first (`Structure → Supercell...`), so that the replaced fraction can be something other than 1/3 or 2/3. To choose specific atoms by hand, select them in the main view and use **Selected atoms** below.

>[!NOTE]
>This operation preserves all atomic positions and lattice parameters — only the element identity changes. The structure's formula is updated to match.

### Mixed Occupancy (Disordered Sites)

For a disordered alloy or solid solution you want each site to hold *both* elements at fractional occupancies (for example Fe₀.₅Ni₀.₅, or O₀.₆₇N₀.₃₃), not a fixed pattern of swapped atoms.

**In the Basis tab**
1. In **Replace an element**, enter *Find* and *Replace with* as usual and choose how many sites to affect (all by default; the count and **Choose at random** work as above).
2. Tick **Mix in as a partial occupancy**, set the **Fraction of Replace-with on each site**, and click **Mix in**.

Each chosen site keeps its element at `1 − fraction` of its occupancy, and the new element takes `fraction`, at the same position. For example, mixing N into every O of BaTiO₃ at 0.33 leaves three sites of O 0.67 / N 0.33.

To mix specific sites by hand, select them in the main view and use **Mix in** under **Selected atoms**.

**What you will see**: the main view and the Structure preview draw a mixed site as a sphere split into sectors by occupancy. Structure Info gains an `Occ` column and the formula carries the fractions (for example `BaN0.33O2.67Ti`).

**What uses the occupancies**: XRD and bond-valence sums weight each species by its occupancy (a virtual-crystal approximation). Supercells copy mixed sites unchanged.

>[!WARNING]
>**Slab generation and primitive/conventional conversion do not support mixed sites** and will refuse with a message rather than return a wrong structure. Build the slab or convert the cell first, then set the mixed occupancies.
>
>**Saving**: POSCAR, QE, XYZ and SPR-KKR have no occupancy field here, so a mixed site is written as two atoms at the same position and a warning is logged. There is no CIF writer yet, so the occupancies cannot currently be saved to a file.

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
