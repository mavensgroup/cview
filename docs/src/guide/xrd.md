# XRD Simulation



The X-Ray Diffraction (XRD) module simulates the powder diffraction pattern for the loaded structure. It utilizes **Kinematic Diffraction Theory**, assuming ideal interactions without primary extinction or multiple scattering events.

## Physical Model

### 1. Lattice Physics
The simulation first calculates the **Reciprocal Lattice** vectors ($b_1, b_2, b_3$) from the real space lattice ($a_1, a_2, a_3$):
$$b_1 = 2\pi \frac{a_2 \times a_3}{a_1 \cdot (a_2 \times a_3)}$$
*(and cyclic permutations for $b_2, b_3$)*.

### 2. Bragg Condition
The code iterates through integer Miller indices $(h, k, l)$ to find reciprocal lattice vectors $G_{hkl} = h b_1 + k b_2 + l b_3$. The d-spacing is calculated as:
$$d_{hkl} = \frac{2\pi}{|G_{hkl}|}$$
Diffraction peaks are identified where the Bragg condition is met for the source wavelength ($\lambda = 1.5406 Å$, Cu K$\alpha$):
$$\lambda = 2d \sin\theta$$

### 3. Structure Factor ($F_{hkl}$)
The intensity of each reflection is governed by the Structure Factor, calculated by summing over all $N$ atoms in the unit cell:
$$F_{hkl} = \sum_{j=1}^{N} f_j(\theta) \exp\left[2\pi i (hx_j + ky_j + lz_j)\right]$$
* $x_j, y_j, z_j$: Fractional coordinates of atom $j$.
* $f_j(\theta)$: The atomic scattering factor.

```admonish info title="Coefficients"
The atomic scattering factor ($f_j(\theta)$) has been calculated using Cromer-Mann coefficient (`model/elements.rs`)
```
<!-- > **Note**: The current implementation approximates $f_j$ using the atomic number ($Z$) or pre-tabulated coefficients compatible with standard crystallographic tables. -->

### 4. Intensity Correction
The raw squared structure factor $|F_{hkl}|^2$ is corrected to obtain the observed intensity $I$. The primary correction applied in `xrd.rs` is the **Lorentz-Polarization (LP) Factor** for unpolarized radiation (standard laboratory diffractometer):

$$
\begin{align}
LP(\theta) &= \frac{1 + \cos^2(2\theta)}{\sin^2\theta \cos\theta}\\\\
I_{calc} &= |F_{hkl}|^2 \times LP(\theta)
\end{align}$$

### 5. Multiplicity and Merging
In powder diffraction, symmetry-equivalent planes (e.g., $(100)$ and $(010)$ in cubic systems) diffract at the same angle. The code algorithmically sorts peaks by $2\theta$ and merges peaks falling within a narrow tolerance ($0.05^\circ$), summing their intensities to account for multiplicity.

### 6. Match with Experiment
You can load the experimental ascii/xlsx file to compare the xrd using the "`Load Experiment`" button.

## Settings
* **Wavelength**: Default fixed to Cu K$\alpha$ ($1.5406 Å$).
* **Range**: $10^\circ$ to $90^\circ$ $2\theta$.
* **Filtering**: Peaks with intensity $\leq 10^{-4}$ relative to the maximum are discarded to reduce noise.
