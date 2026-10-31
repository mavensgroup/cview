# To Do

Planned features, roughly in priority order. Contributions are welcome —
open an issue before starting on one so work isn't duplicated.

## Building and editing structures

- [x] **Build a structure from a space group** and its unique atomic sites
  (asymmetric unit), and edit an existing structure the same way.
- [ ] **Add and move atoms** interactively, with undo.

## Visualization

- [x] **Bond rules per element pair** (minimum/maximum distance, boundary
  handling), shared by bonds, polyhedra and bond valence.
- [ ] **Magnetic moments as arrows**, read from VASP (`MAGMOM`, OUTCAR),
  Quantum Espresso and magnetic CIF.
- [ ] **Thermal ellipsoids** from anisotropic displacement parameters in CIF.
- [ ] Overlay two structures in one view.

## File formats

- [ ] Volumetric data from **XSF/AXSF** and **Gaussian cube**, in the existing
  Charge Density window.
- [ ] Structures from WIEN2k, CASTEP, FHI-aims and ABINIT.

## Analysis

- [ ] Measurements on five or more selected atoms (centroid, plane fit) and
  explicit Distance / Angle / Dihedral buttons.
- [ ] Fermi surfaces from BXSF files.
- [ ] Phonon mode animation.

## Automation

- [ ] Command-line conversion, standardization, k-path and figure export
  without opening the window.
- [ ] Python scripting.
