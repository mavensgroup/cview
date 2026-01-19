# High symmetry _k-points_ and _k-path_

Electronic structure calculations (Band Structures) requires sampling the energy eigenvalues along specific high-symmetry lines in the first Brillouin Zone. This module automates the generation of these paths.

## Implementation

### 1. Brillouin Zone Standardization
The code relies on the **Moyo** library to first determine the Bravais lattice type (e.g., FCC, BCC, HEX). This step is crucial because the definition of "High Symmetry Points" depends entirely on the lattice symmetry.

### 2. High Symmetry Points
CView implements standard K-point definitions (typically following the **Setyawan-Curtarolo** convention).
* **Input**: Real space structure.
* **Transformation**: Converted to Primitive Reciprocal space.
* **Mapping**: Standard points (e.g., $\Gamma = [0,0,0]$, $X = [0, 0.5, 0]$) are generated based on the detected space group.

### 3. Path Generation
The module generates two data sets:
1.  **Linear Path**: A sequence of k-points (e.g., $\Gamma \rightarrow X \rightarrow W \rightarrow K \rightarrow \Gamma$) for band structure plotting.
2.  **Wireframe**: A list of Cartesian line segments representing the edges of the first Brillouin Zone, used for 3D visualization in the UI.

### Coordinate Systems
The internal logic handles the matrix multiplication to convert between:
* **Fractional Reciprocal Coordinates**: Used for DFT inputs (e.g., KPOINTS file).
* **Cartesian Reciprocal Coordinates**: Used for the 3D wireframe visualization in CView.


```admonish warning title="Current Limitations"
While the generated _high-symmetry k-paths_ are crystallographically correct for all space groups (based on the Setyawan-Curtarolo convention), the 3D Brillouin Zone wireframe visualization is currently hardcoded for Cubic systems.

Non-cubic lattices will display correct path data but an incorrect (cubic) boundary box in the 3D viewer.
```

```admonish tip title="Help Wanted: Developers"
We welcome contributions to expand BZ visualization!
If you wish to implement wireframes for other crystal systems, please extend the definitions in:
`src/model/bs_data.rs`
```
