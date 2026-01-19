# Voids Analysis



This module performs geometric analysis to identify empty space within the crystal lattice. It is critical for research into battery materials (ion intercalation), porous frameworks (MOFs/Zeolites), and defect analysis.

## Algorithm: Grid-Based Geometric Insertion

The analysis does not rely on Voronoi decomposition but rather a robust **Grid Probe Method**.

### 1. Grid Generation
A Cartesian grid is superimposed over the unit cell. The resolution is determined dynamically or fixed (standard grid spacing is generally $\leq 0.2 \text{\AA}$ for high precision).
$$P_{grid} = u \cdot a + v \cdot b + w \cdot c \quad \text{where } u,v,w \in [0, 1]$$

### 2. Distance Field Calculation
For every point on the grid, the algorithm calculates the Euclidean distance to the nearest atomic surface. This accounts for Periodic Boundary Conditions (PBC) by checking nearest neighbor images.
$$D_{surf} = \min_{atoms} (||P_{grid} - P_{atom}|| - R_{vdw})$$
Where $R_{vdw}$ is the Van der Waals radius of the atom.

### 3. Probe Insertion
A geometric probe (representing a gas molecule or ion) with radius $R_{probe}$ is tested at each grid point. A point is considered a "Void" if:
$$D_{surf} > R_{probe}$$

### 4. Clustering (Largest Sphere)
To find discrete void centers (e.g., for `max_sphere_center`), the algorithm aggregates contiguous void points. The implementation explicitly identifies the point with the maximum clearance radius to locate the largest cavity center.

## Presets and Data
The module includes standard probe definitions for common applications:
* **Gases**: He ($1.20 Å$), N$_2$ ($1.82 Å$), CO$_2$ ($1.65 Å$).
* **Ions**: Li$^+$ ($0.76 Å$), Na$^+$ ($1.02 Å$), Mg$^{2+}$ ($0.72 Å$).

The void fraction is calculated as:
$$\phi = \frac{N_{void}}{N_{total}} \times 100\%$$
