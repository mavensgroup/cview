# Symmetry Analysis



The Symmetry module identifies the underlying crystallographic space group of the loaded structure. It is essential for reducing computational cost in DFT calculations and validating experimental structures.

## Algorithmic Implementation

CView utilizes the **Moyo** library (a Rust ecosystem equivalent to Spglib) to perform symmetry determination. The analysis pipeline proceeds as follows:

1.  **Lattice Standardization**: The internal `Structure` (Cartesian coordinates) is converted into a `Moyo::Cell`, utilizing the lattice vectors as columns of a $3\times3$ matrix.
2.  **Coordinate Transformation**: Atomic positions are transformed from Cartesian ($r_{cart}$) to Fractional ($r_{frac}$) coordinates via the inverse lattice matrix:
    $$r_{frac} = M_{lattice}^{-1} \cdot r_{cart}$$
3.  **Symmetry Search**: The algorithm searches for symmetry operations (rotations $R$ and translations $t$) that map the crystal onto itself:
    $$R \cdot r + t \equiv r' \pmod 1$$
    where $r$ and $r'$ are atomic positions of the same species.

### Tolerance (`SYMPREC`)
The code applies a default symmetry precision (`SYMPREC`) of **1e-3 Å**. This tolerance accommodates minor numerical noise common in file formats like `.cif` or `.xyz`, ensuring that slightly distorted experimental structures are correctly identified.

## Outputs

The module returns a `SymmetryInfo` struct containing:

- **Space Group Number**: The International Tables for Crystallography (ITA) number (1–230).
- **International Symbol**: The Hermann-Mauguin notation (e.g., $Pm\overline{3}m$, $Fm\overline{3}m$).
- **Crystal System**: The classification (Triclinic, Monoclinic, Orthorhombic, Tetragonal, Trigonal, Hexagonal, or Cubic).

## Usage Reference
This analysis is performed "read-only" regarding the structure; it calculates descriptors without altering the atomic coordinates of the active tab.
