# Surface Slabs (Building)

Creating surface models from bulk crystals is a prerequisite for surface science calculations (catalysis, work function, surface energy). The Slab Builder mathematically transforms the unit cell to expose a specific Miller Index plane $(hkl)$.

## Mathematical Formulation

### 1. Basis Transformation
The core challenge is finding two lattice vectors ($u, v$) that lie perfectly within the plane defined by the normal vector $(hkl)$, and a third vector ($w$) that projects out of the plane.

The algorithm (`find_plane_basis` in `miller_algo`) solves the Diophantine equation to ensure integer linear combinations of the original lattice vectors form the new surface basis. This ensures the surface unit cell area is minimized (primitive surface cell).

### 2. Basis Re-mapping
Once the new basis matrix $M_{surf}$ is found, all atomic positions $r$ are transformed:
$$r_{new} = M_{surf}^{-1} \cdot r_{old}$$
Atoms are then wrapped to lie within the boundaries $[0, 1)$ of the new unit cell.

### 3. Slab Generation
1.  **Replication**: The unit cell is repeated `thickness` times along the new $c$-axis (surface normal).
2.  **Vacuum**: The lattice vector $c$ is elongated by adding `vacuum` distance (in Ångstroms).
    $$c_{final} = c_{slab} + c_{vacuum}$$
    This isolates the slab in the z-direction, preventing spurious interactions between periodic images in DFT calculations.

### Duplicate Removal
The code includes a proximity check (`remove_duplicate_atoms` with `TOLERANCE = 1e-5`) to ensure that atoms lying exactly on cell boundaries are not double-counted during the wrapping process.
