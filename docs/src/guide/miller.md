# Miller Planes

Miller indices $(hkl)$ are a standard notation for describing crystallographic planes. CView allows you to visualize these planes directly in the viewport and use them as input for [slab generation](slabs.md).

---

## Miller Index Notation

A Miller plane $(hkl)$ is defined by its intercepts with the lattice vectors $\mathbf{a}$, $\mathbf{b}$, $\mathbf{c}$:

- The plane intersects the **a**-axis at $\mathbf{a}/h$
- The plane intersects the **b**-axis at $\mathbf{b}/k$  
- The plane intersects the **c**-axis at $\mathbf{c}/l$

**Special cases**:
- $h = 0$: Plane is parallel to the **a**-axis (intersects at infinity)
- Negative indices: Written with a bar, e.g., $(1\bar{1}0)$ for $h=1, k=-1, l=0$

### Common Low-Index Planes

| Plane | Description | Example (Cubic) |
|:---|:---|:---|
| $(100)$ | Perpendicular to a-axis | Cube face |
| $(110)$ | Diagonal through two axes | Edge-sharing |
| $(111)$ | Diagonal through all three axes | Close-packed |

In cubic systems, $(100)$, $(010)$, and $(001)$ are equivalent by symmetry. In lower-symmetry systems (e.g., orthorhombic, hexagonal), they are distinct.

---

## Visualizing Miller Planes

**Access**: `View → Miller Indices...` (`Ctrl + M`)

### How to Use

1. Open the Miller Planes dialog
2. Enter the desired indices $(h, k, l)$
   - Example: `1`, `1`, `1` for the $(111)$ plane
3. Click **Add**

**What you see**:
- A semi-transparent plane rendered in the viewport
- The plane intersects the unit cell edges
- The plane is clipped to its intersection polygon inside the displayed unit cell

>[!TIP]
>The viewport polygon is calculated by intersecting the Miller plane with the unit-cell edges. The related integer-basis routine in `physics/operations/miller_algo.rs` is used for slab construction rather than viewport-plane placement.

### Plane Shape

The visible plane is a **polygon** formed by the intersection of the $(hkl)$ plane with the unit cell boundaries.

**Example**: In a cubic unit cell:
- $(100)$ plane → **Square** (intersects 4 edges)
- $(110)$ plane → **Rectangle**
- $(111)$ plane → **Triangle** (intersects 3 edges)

The shape reflects the actual crystallographic geometry, not just a generic overlay.

---

## Algorithmic Details

### Finding the Plane Basis

CView solves for two in-plane vectors $\mathbf{u}$, $\mathbf{v}$ that:
1. Lie entirely within the $(hkl)$ plane
2. Are expressed as integer combinations of $\mathbf{a}$, $\mathbf{b}$, $\mathbf{c}$
3. Have minimal length (primitive surface cell)

**Implementation**: `find_plane_basis()` in `miller_algo.rs` is used internally for [slab generation](slabs.md), where these vectors become the new in-plane lattice vectors. Viewport-plane rendering instead uses direct plane–cell intersection.

### Plane Position

The viewport dialog creates the conventional intercept plane $hx + ky + lz = 1$ in fractional coordinates. For slab generation, you can specify:
- **Number of atomic layers** to include
- **Vacuum thickness** above and below

---

## Connection to Slab Generation

Miller planes are the primary input for creating surface structures:

1. **Select a plane**: Use the Miller Planes tool to visualize candidates
2. **Generate slab**: `Structure → Slab...` (uses the same $(hkl)$ indices)
3. **Add vacuum**: Specified in Ångströms
4. **Repeat layers**: Control slab thickness

See the [Slab Generation Guide](slabs.md) for details.

---

## Practical Examples

### Example 1: TiO₂ Rutile (110) Surface

The $(110)$ plane of rutile TiO₂ is the most stable surface.

**Workflow**:
1. Load rutile TiO₂ structure
2. Open `View → Miller Indices...`
3. Enter `1`, `1`, `0`
4. Observe the plane cutting through rows of oxygen atoms

This helps you understand which atoms will be exposed in a slab calculation.

### Example 2: Graphene (0001) Plane

For hexagonal structures like graphite, the $(0001)$ plane (also called the "c-plane") is perpendicular to the stacking direction.

**Indices**: Enter `0`, `0`, `1`. CView uses the three-index $(hkl)$ convention.

>[!NOTE]
>For hexagonal systems, convert from Miller-Bravais $(hkil)$ to CView's three-index $(hkl)$ convention before entering indices.

---

## Keyboard Shortcuts

- **Add Plane**: `Ctrl + M`, or `View → Miller Indices...`
- **Clear Plane**: Use **Clear All** in the dialog. Closing or cancelling the dialog leaves existing planes visible.

---

## Related Features

- [Slab Generation](slabs.md): Create vacuum-separated surfaces along $(hkl)$
- [Charge Density](charge_density.md): Slice CHGCAR along Miller planes
- [Symmetry](symmetry.md): Identify symmetry-equivalent planes
