# Analysis Tools

The Analysis module in CView aggregates tools designed to characterize the geometric, symmetric, and diffraction properties of the loaded crystal structure. Unlike simple visualization, these tools perform computational tasks to extract physical descriptors suitable for comparison with experimental data or preparation for *ab initio* calculations.

## Accessing Analysis Tools

The analysis suite is accessible via the **Analysis** menu in the main application window. This triggers the centralized Analysis Window (`actions_analysis.rs`), which provides tabbed access to the following modules:

1.  **Symmetry**: Space group determination and symmetry operation analysis.
2.  **XRD**: X-Ray Diffraction pattern simulation.
3.  **Voids**: Porosity and intercalation site analysis.
4.  **K-Path**: Reciprocal space path generation for band structures.

## Documentation Modules

Detailed physical derivation and algorithmic implementation for each tool are provided below:

* **[Symmetry Analysis](symmetry.md)**
    * *Engine*: Moyo (Rust implementation of Spglib).
    * *Output*: Space Group symbols, numbers, and crystal systems.
* **[XRD Simulation](xrd.md)**
    * *Theory*: Kinematic Diffraction Theory.
    * *Features*: Powder patterns, Cu-K$\alpha$ radiation, Lorentz-Polarization corrections.
* **[Void & Intercalation](voids.md)**
    * *Method*: Grid-based geometric insertion.
    * *Application*: Porosity calculation and battery ion insertion sites.
* **[Reciprocal Space (K-Path)](kpath.md)**
    * *Method*: High-symmetry path standardization (Setyawan-Curtarolo).
    * *Application*: Band structure calculation inputs.

---
*Note: While **Surface Slabs** are technically a structural building operation, their documentation regarding miller index transformation is provided here for completeness.*

* **[Surface Slabs](slabs.md)**
    * *Method*: Lattice transformation via planar basis searching.
