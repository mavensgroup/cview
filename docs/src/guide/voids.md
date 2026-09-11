# Voids Analysis



This module performs geometric analysis to identify empty space within the crystal lattice. It is critical for research into battery materials (ion intercalation), porous frameworks (MOFs/Zeolites), and defect analysis.

## Algorithm: Grid-Based Geometric Insertion

The analysis does not rely on Voronoi decomposition but rather a robust **Grid Probe Method**.

### 1. Grid Generation
A regular fractional grid is superimposed over the unit cell. The configured value is a target grid spacing in Å; the engine default is 0.25 Å and the UI initially uses 0.3 Å. The number of divisions along each axis comes from the *interplanar* spacings ($d_a = V / |b \times c|$, and cyclic), so the configured value is the perpendicular sample spacing whatever the cell shape — for an orthogonal cell $d_a = |a|$.
$$P_{grid} = u \cdot a + v \cdot b + w \cdot c \quad \text{where } u,v,w \in [0, 1]$$

### 2. Distance Field Calculation
For every point on the grid, the algorithm calculates the Euclidean distance to the nearest atomic surface. This accounts for Periodic Boundary Conditions (PBC) by checking nearest neighbor images.
$$D_{surf} = \min_{atoms} (||P_{grid} - P_{atom}|| - R_{vdw})$$
Where $R_{vdw}$ is the Van der Waals radius of the atom.

### 3. Probe Insertion
A geometric probe (representing a gas molecule or ion) with radius $R_{probe}$ is tested at each grid point. A point is considered a "Void" if:
$$D_{surf} > R_{probe}$$

### 4. Largest-Sphere Search
The implementation tracks the sampled point with the greatest clearance from every atomic surface, then refines it off the grid with a compass search on the continuous distance function. A pattern search is used rather than a gradient step because several atoms are equidistant at a maximum, so the gradient is discontinuous there. The reported radius is therefore *not* quantized by the grid spacing — which matters, because at 0.25 Å the quantization error is wide enough to straddle Li$^+$ (0.76 Å) and Mg$^{2+}$ (0.72 Å).

It reports that single global maximum as `max_sphere_center`; it does not cluster connected void regions into separate cavities.

### 5. Interstitial Site Search
The largest sphere answers *how big is the biggest hole*. The site search answers *where are all the holes*, which is the question that matters before an intercalation study.

Every grid point that is no lower than all 26 of its periodic neighbours is a candidate maximum. Plateaus of equal samples are collapsed to one representative through a union-find over grid adjacency, each representative is refined off the grid as above, and representatives that converge onto the same maximum are merged. What survives is filtered against the Shannon radius of the selected candidate ion.

FCC is the reference case: the search returns exactly twelve sites in two families — four octahedral holes at $a/2$ from the nearest atom, and eight tetrahedral at $a\sqrt{3}/4$.

Sites are drawn in both the analysis preview and the main 3D view (**View → Show Interstitial Sites**, `Ctrl+I`) at their fitted radius, so a site that barely admits the ion looks tighter than one with room to spare.

```admonish warning title="This is a geometric screen"
The search reports where a hard sphere fits in the **rigid, as-loaded** framework. It models no electrostatics, no lattice relaxation, and no migration barriers: a site can be geometrically roomy and still be electrostatically impossible, and a real bottleneck opens up under relaxation in ways this does not capture.

Read the output as a screen to run *before* a DFT/NEB calculation, never as a substitute for one.
```

## Performance
Sampling the distance field is the whole cost of the analysis, and both the void numbers and the site list come out of a single sweep rather than two.

Each atom is expanded into its 27 periodic images once, up front, so the minimum-image search disappears from the inner loop, and a uniform cell list reduces that loop to a local neighbourhood scan whose cost does not grow with the number of atoms. On an 18-core machine a 216-atom cell at $0.15\,\text{\AA}$ takes about 80 ms whether it is orthogonal or sheared; a 1000-atom cell over 25M grid points takes about 1.1 s.

The sweep runs on a worker thread, so the window stays responsive, and it is cancelled if you start another before it finishes.

## Presets and Data
The module includes standard probe definitions for common applications:
* **Gases**: He ($1.30 Å$), H$_2$ ($1.45 Å$), H$_2$O ($1.32 Å$), CO$_2$ ($1.65 Å$), N$_2$ ($1.82 Å$), O$_2$ ($1.73 Å$), Ar ($1.70 Å$), Kr ($1.80 Å$), CH$_4$ ($1.90 Å$), and C$_2$H$_6$ ($2.20 Å$).
* **Ions**: Li$^+$ ($0.76 Å$), Na$^+$ ($1.02 Å$), Mg$^{2+}$ ($0.72 Å$).

The void fraction is calculated as:
$$\phi = \frac{N_{void}}{N_{total}} \times 100\%$$
