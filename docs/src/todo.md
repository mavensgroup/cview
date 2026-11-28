# To Do

Planned features, roughly in priority order. Contributions are welcome —
open an issue before starting on one so work isn't duplicated.

## Building and editing structures

- [x] **Build a structure from a space group** and its unique atomic sites
  (asymmetric unit), and edit an existing structure the same way.
- [x] **Add, move and replace atoms** by typed coordinates or at an
  interstitial site, with undo and redo for every structure edit.
- [ ] Drag atoms with the mouse.

## Visualization

- [x] **Bond rules per element pair** (minimum/maximum distance, boundary
  handling), shared by bonds, polyhedra and bond valence.
- [x] **Magnetic moments as arrows**, read from VASP (INCAR `MAGMOM` or
  OUTCAR beside a POSCAR/CONTCAR, vasprun.xml), Quantum Espresso output,
  magnetic CIF (`.mcif`, with the magnetic space-group operations) and
  extended XYZ.
- [x] **Magnetic order**: set moments per site, build antiferromagnetic
  cells from a propagation vector (FM, A-, C-, G-type presets), and write
  them for VASP (`MAGMOM`, in POSCAR order) and Quantum Espresso (one
  species per moment, `starting_magnetization`).
- [x] **Spin textures** (spirals, skyrmions): colour by direction, arrows
  without atoms, and Quantum Espresso input that stays within pw.x's
  species limit.
- [x] **OVF files** (mumax3, OOMMF, Spirit): magnetization textures shown
  as arrows.
- [x] **OVF snapshot series** play in the Trajectory Player, with moment
  arrows and a colour map.
- [x] Topological charge Q and skyrmion radius per frame in the Trajectory Player.
- [x] **DFT+U orbitals** in the 3D charge density window: occupied orbitals or
  shell spin density from the OUTCAR occupation matrices, drawn on their atoms.
- [x] **Defect figures** in the 3D charge density window: crop box, bonds,
  markers (cube, ring, dot) with labels placed clear of the atoms, atom
  highlights, and figures rendered from a scene file without the GUI.
- [ ] Build skyrmion lattices; m_z = 0 isosurface for thick films.
- [ ] Magnetic moment arrows in the 3D charge density window.
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
