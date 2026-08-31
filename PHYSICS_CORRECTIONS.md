# CView Physics-Correctness Update — Change Brief & Paper Guidance

Prepared 2026-07-20, after completing the full physics-correctness review (all CRITICAL,
HIGH, MEDIUM, and LOW findings resolved or explicitly deferred). CView v0.9.x, 90 unit
tests passing. This document is self-contained: Part 1 is the technical changelog,
Part 2 tells the manuscript writer exactly what the paper may now claim, what it must
delete, and gives adaptable method-section text.

---

## Part 1 — Change brief

### XRD (powder pattern simulation)

- **Cromer–Mann form factors implemented.** The atomic form factor is no longer the
  constant f₀ = Z; it is the 4-Gaussian analytic expansion
  f₀(s) = Σᵢ aᵢ exp(−bᵢs²) + c evaluated at s = sinθ/λ, using the coefficient table of
  International Tables for Crystallography Vol. C, Table 6.1.1.4 (105 elements).
  Validation: Si relative intensities I(220) = 61.7, I(311) = 34.9 vs ICDD reference
  55 and 30 (residual gap is the generic B = 1.0 default; with f₀ = Z they were
  overestimated 2–3×). Evaluated once per element per reflection (not per atom).
- **Reflection enumeration is now exact.** The hkl loop bounds scale per axis as
  ⌈g_max·|aᵢ|⌉+1 with g_max = 2sin(θ_max)/λ (previously a fixed ±6 cube that silently
  truncated the high-angle half of the pattern for cells ≳ 7 Å). A ±50 safety cap warns
  in the console when it binds.
- **Peak merging** places merged peaks at the intensity-weighted mean 2θ; the weak-peak
  threshold is relative (10⁻⁴ of the strongest peak), not absolute.
- **Occupancy weighting** (virtual-crystal approximation): f_eff = f₀·occupancy.
  Regression test: the extinct bcc (100) reflection appears when the body-center site
  is half-occupied.
- Multiplicity is obtained by enumeration and merging (symmetry-equivalent reflections
  coincide exactly); accidentally coincident non-equivalent families (cubic (333)/(511))
  merge into one peak — standard for a powder tool, documented.

### Band path / Brillouin zone (Setyawan–Curtarolo 2010)

- **Convention bug underneath everything:** the moyo library stores lattice vectors as
  matrix *columns* (`Lattice::new` transposes its row input); the code assumed rows.
  Every extracted conventional-cell parameter and every Cartesian k-point/BZ vertex was
  computed from a transposed (wrong-metric) cell for non-orthogonal lattices. Fixed.
  The same transpose bug existed in the Primitive⇄Conventional conversion tool (it
  returned transposed lattices for hexagonal/monoclinic/rhombohedral cells). Fixed with
  a hexagonal-metric regression test.
- **Rhombohedral (KP-1/KP-2):** R space groups are classified and parameterized from the
  rhombohedral cell (converted analytically from spglib's triple hexagonal cell), not
  from the hexagonal cell's 90° angles. Bi₂Se₃ (R-3m) now classifies RHL1 with
  η = 0.8229, ν = 0.3386. The RHL2 η formula was corrected to 1/(2tan²(α/2)).
- **Monoclinic (KP-3/KP-4):** ITA unique-axis-b parameters are mapped onto the SC
  convention (oblique angle → α < 90°, b ≤ c for MCL). MCLC1–5 sub-classification uses
  kγ of the reciprocal of the MCLC *primitive* cell (the conventional cell's kγ is
  identically 90°, so every base-centred monoclinic previously landed in MCLC2).
  Baddeleyite ZrO₂ (P2₁/c) now yields MCL with non-degenerate H = (0, 0.4335, 0.5718).
- **Additional formula errors found beyond the review** (verified against pymatgen and
  AFLOW): MCLC3 μ used b²/c² instead of b²/a²; MCLC5 ζ and μ had the same a↔c swaps.
- **Orthorhombic (KP-5/KP-6):** A-centred groups (SG 38–41) are cyclically permuted to
  C-centring; base-centred cells enforce SC's a < b. ORCI L₂ corrected to (½−δ, ½+δ, −μ).
- **Basis consistency by construction (KP-7):** the reciprocal basis is built from the
  SC primitive vectors constructed explicitly per the SC10 definitions for all 14
  Bravais lattices — correctness no longer depends on any symmetry-library internals.
- Regression suite: Si (FCC, |X| = 2π/a), Mg (HEX, |K| = 4π/3a), α-U (ORCC, ζ = 0.3091),
  Bi₂Se₃ (RHL1), ZrO₂ (MCL), synthetic MCLC1/3/5 branch tests, primitive-cell volume
  identities.
- Hygiene: unified symmetry tolerance (10⁻⁴) across symmetry tab / k-path / conversion;
  Hermann–Mauguin symbols shown (SG 112 typo "P42c" → "P-42c" fixed); exported KPOINTS
  header states the coordinates are valid only for the standardized primitive cell;
  triclinic cells with mixed-sign reciprocal angles produce a console warning.

### Bond-valence sums

- **The empirical fallback is now the real O'Keeffe–Brese scheme.** Previously the
  "Brese–O'Keeffe fallback" was a different formula (Shannon ionic radii + Pauling χ
  + |valence| weights) that overestimated R₀ by 0.2–0.5 Å, inflating bond valences
  2–4×. Now: R₀ = rᵢ + rⱼ − rᵢrⱼ(√cᵢ−√cⱼ)²/(cᵢrᵢ + cⱼrⱼ) with the authors' fitted
  (r, c) atomic parameters. Every parameter row was validated against the tabulated R₀
  of Brese & O'Keeffe (1991), Tables 2–3; agreement 0.001–0.06 Å. Elements failing
  validation (Cu, Au, Pd, Rh, In, Sn, Sb, most f-block) have **no** fallback — such
  pairs are skipped and reported "n/a" rather than computed with a wrong R₀.
- **Per-site role resolution for amphoteric elements** (H, N, P, As, S, Se, Te) by
  nearest-neighbour electronegativity: BaSO₄ → S⁶⁺ (BVS ≈ 6), ZnS → S²⁻, NaNO₃ → N⁵⁺,
  Li₃N → N³⁻, H₂O → H⁺, NaH → H⁻. Polyanionic chemistry (sulfates, phosphates,
  nitrates, arsenates) now yields correct sums; previously the central atom was tagged
  as an anion and all its bonds were skipped.
- **Single unit cell is sufficient input:** interatomic offsets are minimum-image
  wrapped before periodic-image enumeration, with the range bound corrected to
  ⌈cutoff/dᵢ + ½⌉. Atoms at fractional 1.0 or unwrapped coordinates give bit-identical
  results; unit cell ≡ supercell verified by test.
- **Parameter provenance surfaced** per atom: exact IUCr entry ("IUCr"), substituted
  valence of the same pair ("IUCr*"), or O'Keeffe–Brese estimate ("B&OK").
- Quality banner is banded on the **GII** (< 0.1 stable, > 0.2 strained — Salinas-
  Sánchez 1992; Brown 2002), not on mean |Δ|. Five typo'd keys in the bvparm2020-derived
  table fixed. The report's speculative "recommendations" were replaced by purely
  factual notes about the calculation (interpretation of deviations is context-
  dependent: slab surfaces are legitimately under-bonded; ideal cubic perovskites are
  genuinely strained — verified: cubic BaTiO₃ gives Ba +0.74 / Ti −0.38, GII 0.385,
  the textbook bond-strain precursor of its ferroelectric instability).
- **Occupancy weighting:** each neighbour's bond valence is scaled by its occupancy
  (mean-field); the central atom's own occupancy does not scale its BVS (an occupied
  site wants full valence).

### Site occupancy (new model capability)

- `Atom.occupancy` added to the data model (default 1.0). CIF reads
  `_atom_site_occupancy` and **keeps split sites** (coincident positions with different
  elements) instead of dropping the second species; the CIF writer round-trips the
  occupancy column. SPR-KKR CPA sites (NOQ > 1, e.g. Fe₀.₇Cr₀.₃) import as coincident
  atoms with occupancy = concentration (previously only the majority species survived).
  Formats without an occupancy concept (POSCAR/XYZ/QE) warn on export.

### Geometry operations & other analyses

- **Slab (S-1):** vacuum insertion keeps Cartesian atomic positions fixed (computed in
  the pre-vacuum cell) — the previous fractional-z rescaling sheared every slab whose
  stacking vector is oblique to the surface, changing interlayer bond lengths. Test: a
  rutile (101) slab is bit-identical with 0 and 15 Å vacuum. Non-coprime Miller indices
  ((200), (220)) reduce by gcd instead of failing (S-2).
- **Supercell transform:** fixed a transpose error — new fractional coordinates require
  (Mᵀ)⁻¹, not M⁻¹, in the row-vector convention; cyclic axis permutations and shears
  previously placed atoms at wrong positions. Search ranges in slab and supercell are
  now exact per-axis corner-mapped bounding boxes (S-3); tests assert atom count =
  |det|·N and that every output atom lies on the original crystal lattice.
- **Voids (V-1/V-2):** minimum-image distances in oblique cells scan the 27 neighbouring
  offsets (orthogonal cells skip the cost); He probe radius set to 1.30 Å (kinetic
  diameter 2.60 Å, consistent with the N₂/CO₂/CH₄ entries).
- **Polyhedral distortion (PM-1/PM-2):** quadratic-elongation reference constants are
  exact closed forms (tetrahedron d₀/V^⅓ = √3·(3/8)^⅓ = 1.24902; the old 1.2408 biased
  every tetrahedral ⟨λ⟩ by +1.3%). CN 12 reports no ⟨λ⟩: Robinson 1971 defines the
  metric for tetrahedra/octahedra and the CN-12 reference is ambiguous (icosahedron vs
  cuboctahedron differ ~10%). Bond-angle variance σ² documented as the Robinson
  edge-angle convention (what VESTA reports). Perfect polyhedra give ⟨λ⟩ = 1 to 10⁻⁶.
- **Charge-density slices (CD-1):** fractional-axis slices of oblique cells annotate the
  in-plane angle and that the orthogonal-frame map is sheared (e.g. hexagonal (001),
  γ = 120°).

---

## Part 2 — What the paper may claim (guidance for the manuscript)

### XRD section — rewrite required

DELETE: any sentence describing the form factor as "under development", any promise of
Waasmaier–Kirfel parameters in a follow-up note, and any f₀ = Z description.

The paper MAY now state (adapt freely):

> Powder patterns are simulated from the structure factor with atomic form factors
> represented by the four-Gaussian Cromer–Mann analytic expansion
> f₀(s) = Σᵢ₌₁⁴ aᵢ exp(−bᵢs²) + c, with coefficients taken from International Tables
> for Crystallography, Vol. C, Table 6.1.1.4 [cite ITC Vol. C; optionally Cromer &
> Mann, Acta Cryst. A24, 321 (1968)]. Reflections are enumerated to the exact
> resolution limit implied by the user-selected angular range, with per-axis bounds
> derived from the cell metric. Thermal motion is modelled by a single isotropic
> Debye–Waller factor, and intensities include the standard Lorentz–polarization
> correction. Reflection multiplicities emerge from enumeration and merging of
> symmetry-equivalent reflections rather than from point-group lookup tables. Site
> occupancies weight the form factors (virtual-crystal approximation), so
> substitutionally disordered and split-site structures produce correct Bragg
> intensities. Simulated patterns for Si reproduce ICDD reference peak positions to
> < 0.05° 2θ and relative intensities to within the uncertainty of the global
> thermal parameter.

Limitations to state honestly (one sentence is enough): neutral-atom form factors (no
ionic species, no anomalous dispersion f′/f″), single global B, Kα₁ only, constant-width
display broadening (no Caglioti profile), no diffuse scattering / short-range order.

### Band-structure path section

The paper MAY claim: full Setyawan–Curtarolo (2010) [cite: W. Setyawan, S. Curtarolo,
Comput. Mater. Sci. 49, 299 (2010)] high-symmetry point and path generation for all 14
Bravais lattices including all parameter-dependent sub-variants (BCT1/2, ORCF1–3, RHL1/2,
MCL, MCLC1–5, TRI1/2); classification from the symmetry-standardized cell (moyo/spglib
conventions [cite spglib: Togo & Tanaka, arXiv:1808.01590; moyo if it has a citation])
with an explicit mapping onto the SC conventions; the reciprocal basis is constructed
directly from the SC primitive-cell definitions so fractional k-point coordinates and
the Brillouin-zone geometry are mutually consistent by construction. Validated against
reference structures spanning cubic, hexagonal, rhombohedral (Bi₂Se₃, RHL1),
base-centred orthorhombic (α-U) and monoclinic (baddeleyite ZrO₂) classes.

Note for the text: exported k-point coordinates refer to the standardized primitive
cell; the KPOINTS header says so and the GUI offers primitive-cell conversion.

### Bond-valence section

The paper MAY claim: BVS with parameters from the IUCr bvparm2020 compilation [cite:
I. D. Brown, bond-valence parameter file bvparm2020, IUCr; plus Brown & Altermatt, Acta
Cryst. B41, 244 (1985)]; untabulated pairs estimated by the O'Keeffe–Brese
correlation [cite: O'Keeffe & Brese, J. Am. Chem. Soc. 113, 3226 (1991); Brese &
O'Keeffe, Acta Cryst. B47, 192 (1991)] with the parameter provenance (tabulated /
substituted-valence / estimated) reported per atom; per-site cation/anion role
resolution for amphoteric elements so polyanionic compounds (sulfates, phosphates,
nitrates) are handled without user input; explicit oxidation states from the input file
always take precedence; full periodic-image summation such that a single unit cell —
regardless of coordinate wrapping — gives converged sums identical to any supercell;
global instability index GII = √⟨Δ²⟩ with quality bands per Salinas-Sánchez et al. and
Brown [cite: A. Salinas-Sánchez et al., J. Solid State Chem. 100, 201 (1992); I. D.
Brown, The Chemical Bond in Inorganic Chemistry, IUCr Monograph 12, OUP (2002)];
occupancy-weighted neighbour contributions (mean-field) for disordered structures.

Good validation anecdote available if useful: cubic BaTiO₃ yields the textbook
bond-strain signature (Ba over-bonded +0.74 v.u., Ti under-bonded −0.38 v.u.,
GII = 0.385) — the classic bond-valence rationale for its ferroelectric instability;
this is correct physics, not an error metric.

### Disorder support (can be a selling point)

The paper MAY claim: the data model carries per-site occupancies; CIF split sites are
preserved as coincident partial sites; SPR-KKR CPA occupation blocks import with
concentration-weighted sites; XRD and BVS weight contributions accordingly
(virtual-crystal / mean-field level). State the limitation: no short-range order, local
relaxation, or diffuse scattering — site-averaged quantities only.

### Polyhedral metrics

Claim Robinson quadratic elongation ⟨λ⟩ and bond-angle variance σ² [cite: Robinson,
Gibbs & Ribbe, Science 172, 567 (1971)] using exact regular-polyhedron references for
CN 4/6/8, with σ² over polyhedron edge angles (the convention VESTA reports); ⟨λ⟩ is
deliberately not reported for CN 12 (no unambiguous regular reference). Baur distortion
index [cite: Baur, Acta Cryst. B30, 1195 (1974)] and divergence-theorem volumes as before.

### Do NOT claim anywhere

Ionic/charged form factors; anomalous dispersion; Kα₂ doublets or profile refinement;
occupancy round-trips through POSCAR/XYZ/QE (CIF only); ⟨λ⟩ for CN 12; treatment of
short-range order or local relaxations in disordered phases; Niggli reduction for
triclinic k-paths (a console warning covers the mixed-angle case).

### Citation checklist

1. Setyawan & Curtarolo, Comput. Mater. Sci. 49, 299 (2010) — k-paths
2. Int. Tables for Crystallography Vol. C, Table 6.1.1.4 — form factors
   (replaces the previously planned Waasmaier & Kirfel 1995 citation)
3. Brown & Altermatt, Acta Cryst. B41, 244 (1985) — BVS
4. Brese & O'Keeffe, Acta Cryst. B47, 192 (1991) — BVS parameters
5. O'Keeffe & Brese, J. Am. Chem. Soc. 113, 3226 (1991) — (r, c) estimation scheme
6. IUCr bvparm2020 parameter file — tabulated pairs
7. Salinas-Sánchez et al., J. Solid State Chem. 100, 201 (1992) — GII bands
8. Brown, IUCr Monograph 12, OUP (2002) — bond-valence model
9. Robinson, Gibbs & Ribbe, Science 172, 567 (1971) — ⟨λ⟩, σ²
10. Baur, Acta Cryst. B30, 1195 (1974) — distortion index
11. Shannon, Acta Cryst. A32, 751 (1976) — radii (display/voids)
12. spglib / moyo — symmetry standardization
