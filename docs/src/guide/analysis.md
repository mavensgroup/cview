# Analysis Tools

The Analysis module in CView aggregates tools designed to characterize the geometric, symmetric, and diffraction properties of the loaded crystal structure. Unlike simple visualization, these tools perform computational tasks to extract physical descriptors suitable for comparison with experimental data or preparation for *ab initio* calculations.

## Accessing Analysis Tools

The analysis suite is accessible via the **Analysis** menu in the main application window. Each menu entry opens the Analysis window (`actions_analysis.rs`) directly on its own tab, and you can switch between tabs from within the window:

1.  **Symmetry** (`Analysis → Symmetry...`): Space group determination and symmetry operation analysis.
2.  **XRD** (`Analysis → Diffraction (XRD)...`): X-Ray Diffraction pattern simulation.
3.  **Band Path** (`Analysis → Band Path...`): Reciprocal space path generation for band structures.
4.  **Void Analysis** (`Analysis → Void Analysis...`): Porosity and intercalation site analysis.

**Charge Density** is opened separately from **Analysis → Charge Density**; it is not a tab in the Analysis window.

**Slab generation** changes the structure, so it is not part of the read-only Analysis window. It has its own window under **Structure → Slab...**; see [Surface Slabs](slabs.md).

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
* **[Surface Slabs](slabs.md)**
    * *Method*: Lattice transformation via planar basis searching.
