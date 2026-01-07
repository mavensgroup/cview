# cview

**cview** is a high-performance, lightweight crystal structure viewer written in Rust. It is designed for researchers working with DFT codes (VASP, Quantum Espresso, SPRKKR) who need a fast way to visualize structures, check relaxation results, and simulate XRD patterns.

---

## 🚀 Getting Started

### Installation

Currently, **cview** is distributed as a source package. You will need the Rust toolchain installed.

```bash
# Clone the repository
git clone https://github.com/your-username/cview.git
cd cview

# Run the application
cargo run --release

```

### System Requirements

* **OS:** Linux, Windows, or macOS
* **Graphics:** A GPU supporting Vulkan or OpenGL (almost any modern computer).

---

## 📂 Supported File Formats

**cview** automatically detects file types based on extension and content.

| Code / Format | Extensions | Notes |
| --- | --- | --- |
| **VASP** | `POSCAR`, `CONTCAR`, `.vasp` | Standard VASP structure files. |
| **Quantum Espresso** | `.in`, `.pwi`, `.qe` | Reads `CELL_PARAMETERS` and `ATOMIC_POSITIONS`. |
| **QE Output** | `.out`, `.log` | **Relaxation Aware:** Automatically extracts the *final* relaxed structure from `vc-relax` or `relax` logs. |
| **CIF** | `.cif` | Standard Crystallographic Information File. |
| **XYZ** | `.xyz` | Supports standard XYZ and **Extended XYZ** (Lattice line in comment). |
| **SPRKKR** | `.inp`, `.sys`, `.pot` | Support for Munich SPR-KKR input files. |

* QE and XYZ is not working very well at the moment
---

## 🖥️ Using the Interface

### 1. Opening a Structure

* Go to **File > Open** (or use the toolbar button).
* Select your file.
* *Note:* If you don't see your file, ensure the file extension matches the supported list above.


* The structure will appear in the 3D viewport.

### 2. Navigation Controls

* **Rotate:** Left Click + Drag
* **Pan:** Right Click + Drag (or Shift + Left Click)
* **Zoom:** Scroll Wheel

### 3. Visualizing Relaxation Results (Quantum Espresso)

When you load a Quantum Espresso output log (`.out` or `.log`):

* **cview** scans the entire file.
* It identifies every `atomic_positions` and `cell_parameters` block.
* It displays the **last valid structure** found in the file.
* *Tip:* This allows you to instantly visualize the final geometry of a `vc-relax` calculation without manually extracting data.



### 4. Simulated XRD (X-Ray Diffraction)

**cview** includes a built-in XRD simulator to check phase purity or compare with experimental data.

1. Load a structure.
2. Switch to the **"Simulated XRD"** tab on the right panel.
3. Adjust parameters:
* **Wavelength:** Default is Cu K-alpha ().
* **Min/Max 2θ:** Range of the plot.
* **Smoothing (FWHM):** Adjust peak broadness.


4. The plot updates automatically. You can verify peak positions and relative intensities.

---

## ❓ Troubleshooting

**Q: I cannot see my file in the Open dialog.**
**A:** The file filter might be hiding it. Select "All Supported Files" in the dropdown, or ensure your file has a standard extension (e.g., rename `my_output` to `my_output.out`).

**Q: The atoms look like a single clump/point.**
**A:** This usually happens if the file unit was misread (e.g., Bohr vs Angstrom) or the Lattice is missing.

* For **XYZ** files: Ensure you have a Lattice defined in the comment line, otherwise a default  box is used.
* For **QE**: Ensure `celldm(1)` or `A` is defined in the file.

**Q: The program crashes on startup.**
**A:** Run the program from a terminal using `RUST_BACKTRACE=1 cargo run` to see the error message. Please report this if it persists.

---

## 📜 License

**cview** is free software: you can redistribute it and/or modify it under the terms of the **GNU Lesser General Public License** as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

**cview** is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Lesser General Public License for more details.

You should have received a copy of the GNU Lesser General Public License along with this program. If not, see [https://www.gnu.org/licenses/](https://www.gnu.org/licenses/).
