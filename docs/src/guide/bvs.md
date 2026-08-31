# Bond Valence Sum (BVS)

The Bond Valence Sum (BVS) module is an empirical analytical tool used to estimate the oxidation states of atoms in a crystal structure and evaluate the physical plausibility of their coordination environments.

## Physical Model

The core principle of BVS is that the valence of an atom ($V_i$) is equal to the sum of the individual bond valences ($s_{ij}$) from all its surrounding nearest neighbors:
$$BVS_i = \sum_{j} s_{ij}$$

The individual bond valences are calculated using the standard exponential relationship:
$$s_{ij} = \exp\\left(\\frac{R_0 - R_{ij}}{B}\\right)$$
Where:
* $R_{ij}$: The actual measured distance between atom $i$ and atom $j$.
* $R_0$: The tabulated ideal bond length for the specific atom pair and oxidation state.
* $B$: An empirical constant, typically $0.37 \text{\\AA}$.



### Parameter Database
CView does not require you to input $R_0$ parameters manually. The internal engine strictly implements:
1.  **Primary Database**: The modern **IUCr 2020** (`bvparm2020.cif`) table containing over 1000 oxidation-state-specific element pairs.
2.  **Fallback Database**: The empirical Brese & O'Keeffe (1991) parameters for untabulated pairs.

## Algorithmic Implementation

### Full Periodic Image Search
Unlike basic implementations that use a simple minimum-image convention (which can fail for high-coordination sites like Barium in perovskites where $CN=12$), CView loops over **all** periodic images within a defined spherical cutoff. This ensures mathematically rigorous summation for complex and tightly packed lattices.

### Automatic Valence Matching
Valences are not hardcoded. The algorithm iteratively tests a sequence of plausible valences from the IUCr table (prioritizing the most specific matches first, followed by average sentinel values) to find the best fit for your structure.

---

## Using BVS in CView

### Accessing BVS Values

**Location**: Sidebar → **Bond Valence** expander

Select **Bond Valence** from **Sidebar → Bond Valence → Color Mode**. CView then computes BVS for the loaded structure, applies BVS colours in the viewport, and writes a detailed per-atom report to the Structure Info panel.

**Output format** (per atom):
```
Idx   Elem   Ox      BVS   Expect        Δ   CN  Status
0     Fe     +3    2.930    3.000   -0.070    6  ✓ Excel
1     O      -2    1.980    2.000   -0.020    6  ✓ Excel
```

### BVS Color Mode

**How to enable**: Sidebar → **Bond Valence** → **Color Mode** dropdown → Select **Bond Valence**

**Effect**: Atoms are coloured by the magnitude of their BVS deviation:
- **Green**: Good agreement with the inferred expected valence
- **Yellow/orange**: Intermediate deviation
- **Red**: Large deviation

The colour does not encode the sign of the deviation, so it does not distinguish under-bonding from over-bonding.

This heatmap is particularly useful for:
- Identifying structurally strained sites in defective crystals
- Validating relaxed DFT geometries (all sites should be near-green)
- Spotting charge transfer in interfaces or heterostructures

>[!TIP]
>Combine BVS coloring with [polyhedra visualization](visualization.md#coordination-polyhedra) to simultaneously see coordination geometry and electronic state.
