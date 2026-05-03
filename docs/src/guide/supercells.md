# Supercells

A **supercell** is an expanded version of the unit cell, created by replicating the structure along the lattice vectors. This is essential for modeling defects, surfaces, interfaces, and finite-size effects in *ab-initio* calculations.

---

## What is a Supercell?

The supercell transformation multiplies the lattice vectors by integer factors:

$$
\mathbf{a}_{super} = N_a \cdot \mathbf{a}, \quad
\mathbf{b}_{super} = N_b \cdot \mathbf{b}, \quad
\mathbf{c}_{super} = N_c \cdot \mathbf{c}
$$

All atoms in the original unit cell are replicated at positions:
$$
\mathbf{r}_{new} = \mathbf{r}_{original} + n_a\mathbf{a} + n_b\mathbf{b} + n_c\mathbf{c}
$$
where $n_a \in [0, N_a-1]$, $n_b \in [0, N_b-1]$, $n_c \in [0, N_c-1]$.

**Result**: A structure with $(N_a \times N_b \times N_c)$ times more atoms than the original unit cell.

---

## When to Use Supercells

### 1. **Point Defects**

To model vacancies, interstitials, or substitutional dopants, you need sufficient spacing between periodic images to avoid spurious interactions.

**Example**: A single oxygen vacancy in TiO₂:
- Unit cell: 6 atoms → Too small (vacancy concentration = 16.7%)
- 2×2×3 supercell: 72 atoms → Realistic dilute defect (1.4%)

**Rule of thumb**: Aim for at least 10 Å separation between defects.

### 2. **Surface Calculations**

When combined with [slab generation](slabs.md), supercells allow you to model surface reconstructions, adsorbates, or step edges without artificial periodicity.

**Typical workflow**:
1. Create a 1×1×1 slab (surface + vacuum)
2. Expand to 2×2×1 or 3×3×1 supercell
3. Add adsorbate at specific site

### 3. **Alloying & Disorder**

To simulate random alloys (e.g., Ni₀.₅Co₀.₅) or partially occupied sites, you need enough atoms to represent the composition statistically.

**Example**: 50% Ni / 50% Co substitution:
- 2×2×2 FCC supercell: 32 atoms → 16 Ni + 16 Co

### 4. **Phonons & Finite-Temperature**

Phonon calculations (via DFPT or frozen phonons) often require supercells to sample specific q-points or to avoid interactions between atomic displacements.

---

## Using the Supercell Tool

**Access**: `Tools → Supercell` (or from the application menu)

### Interface

The supercell dialog presents three integer input fields:

- **N_a**: Repetitions along the **a**-axis
- **N_b**: Repetitions along the **b**-axis  
- **N_c**: Repetitions along the **c**-axis

**Default**: `1×1×1` (no expansion)

### Workflow

1. Load your structure (e.g., a primitive cell)
2. Open `Tools → Supercell`
3. Enter desired dimensions (e.g., `2`, `2`, `3`)
4. Click "Generate Supercell"
5. The new structure replaces the current tab

>[!TIP]
>The supercell is generated instantly for typical dimensions (up to ~5×5×5). For very large expansions, expect a few seconds of computation.

### Example: BaTiO₃ 2×2×2 Supercell

- **Original**: 5 atoms (Ba, Ti, 3×O) in perovskite unit cell
- **After 2×2×2**: 40 atoms
- **Use case**: Model a single Ti → Zr substitutional defect

You can now use the [Basis Operations](building.md) to selectively replace one Ti atom with Zr.

---

## Performance Considerations

### Memory Limits

CView is optimized for structures up to **~5000 atoms**. Beyond this, rendering may slow down:

| Supercell Size | Atoms (BaTiO₃) | Performance |
|:---|---:|:---|
| 2×2×2 | 40 | Instant |
| 5×5×5 | 625 | Smooth |
| 10×10×10 | 5000 | Acceptable |
| 20×20×20 | 40000 | Not recommended |

>[!CAUTION]
>CView uses CPU-based rendering. For molecular dynamics trajectories or nanoparticles with >10,000 atoms, consider specialized tools like OVITO or VMD.

### DFT Calculation Cost

Remember that DFT cost scales as $O(N^3)$ with atom count. A 2×2×2 supercell (8× atoms) will be **~500× slower** than the primitive cell.

**Optimization strategy**:
1. Always use the smallest supercell that captures your physics
2. For defects: Check convergence of formation energy with supercell size
3. For surfaces: 2×2 or 3×3 is typically sufficient

---

## Output Formats

After generating a supercell, you can export it via `File → Save Structure As`:

- **VASP (POSCAR)**: Commonly used for DFT
- **Quantum Espresso**: Automatically adjusts `nat` parameter
- **CIF**: For archival or database submission

The lattice vectors are correctly scaled, and all atomic coordinates are in fractional form.

---

## Related Tools

- [Slab Generation](slabs.md): Create surfaces by cutting along Miller planes
- [Basis Operations](building.md): Modify atoms after supercell creation
- [Symmetry Detection](symmetry.md): Check if expansion breaks symmetry
