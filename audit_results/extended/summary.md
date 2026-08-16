# CView corpus round-trip fidelity audit

Files audited: **20000** (seed 42, per-file timeout 60 s)

## Round trip A — CIF → CIF

### All files

| Outcome | Files | % |
|---|---:|---:|
| `PARSE_DEGENERATE_ORIG` | 1 | 0.0 |
| `COUNT_MISMATCH` | 2 | 0.0 |
| `SYMMETRY_CHANGED` | 1 | 0.0 |
| `GEOM_MISMATCH` | 1 | 0.0 |
| `SYMMETRY_UNDETERMINED_BOTH` | 1813 | 9.1 |
| `PASS` | 18182 | 90.9 |

### Inorganic only

| Outcome | Files | % |
|---|---:|---:|
| `PARSE_DEGENERATE_ORIG` | 1 | 0.0 |
| `COUNT_MISMATCH` | 1 | 0.0 |
| `SYMMETRY_CHANGED` | 1 | 0.0 |
| `GEOM_MISMATCH` | 1 | 0.0 |
| `SYMMETRY_UNDETERMINED_BOTH` | 1712 | 32.4 |
| `PASS` | 3573 | 67.6 |

### Organic / metal-organic only

| Outcome | Files | % |
|---|---:|---:|
| `COUNT_MISMATCH` | 1 | 0.0 |
| `SYMMETRY_UNDETERMINED_BOTH` | 101 | 0.7 |
| `PASS` | 14609 | 99.3 |

## Round trip B — CIF → POSCAR → CIF

### All files

| Outcome | Files | % |
|---|---:|---:|
| `PARSE_DEGENERATE_ORIG` | 1 | 0.0 |
| `COUNT_MISMATCH` | 2 | 0.0 |
| `SYMMETRY_CHANGED` | 1 | 0.0 |
| `GEOM_MISMATCH` | 1 | 0.0 |
| `LOSS_OCCUPANCY_EXPECTED` | 4946 | 24.7 |
| `LOSS_OXIDATION_EXPECTED` | 423 | 2.1 |
| `SYMMETRY_UNDETERMINED_BOTH` | 28 | 0.1 |
| `PASS` | 14598 | 73.0 |

### Inorganic only

| Outcome | Files | % |
|---|---:|---:|
| `PARSE_DEGENERATE_ORIG` | 1 | 0.0 |
| `COUNT_MISMATCH` | 1 | 0.0 |
| `SYMMETRY_CHANGED` | 1 | 0.0 |
| `GEOM_MISMATCH` | 1 | 0.0 |
| `LOSS_OCCUPANCY_EXPECTED` | 2198 | 41.6 |
| `LOSS_OXIDATION_EXPECTED` | 373 | 7.1 |
| `SYMMETRY_UNDETERMINED_BOTH` | 28 | 0.5 |
| `PASS` | 2686 | 50.8 |

### Organic / metal-organic only

| Outcome | Files | % |
|---|---:|---:|
| `COUNT_MISMATCH` | 1 | 0.0 |
| `LOSS_OCCUPANCY_EXPECTED` | 2748 | 18.7 |
| `LOSS_OXIDATION_EXPECTED` | 50 | 0.3 |
| `PASS` | 11912 | 81.0 |

## Timing (single-file latency, warm cache, release build)

| Quantity | Median | p95 | Max |
|---|---:|---:|---:|
| `load_structure` parse (ms) | 0.26 | 1.27 | 47.36 |
| Round trip A total (ms) | 0.65 | 2.35 | 81.59 |
| Round trip B total (ms) | 1.23 | 4.42 | 122.17 |

Worst-case parse: COD **2311717** — 47.4 ms, 103418 bytes, 10240 atoms, reflection block: true.

## Notable failures

### Round trip A

**`PARSE_DEGENERATE_ORIG`** — 1 file(s)

- COD 3000271 — sg →, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å — degenerate lattice (volume 0.000e0)

**`COUNT_MISMATCH`** — 2 file(s)

- COD 2108792 — sg 5→5, max Δlen 0.00e0 Å, max Δsite -1.00e0 Å
- COD 3000042 — sg 194→6, max Δlen 0.00e0 Å, max Δsite -1.00e0 Å

**`SYMMETRY_CHANGED`** — 1 file(s)

- COD 9017201 — sg 1→5, max Δlen 0.00e0 Å, max Δsite 5.31e-6 Å

**`GEOM_MISMATCH`** — 1 file(s)

- COD 2107333 — sg 8→8, max Δlen 0.00e0 Å, max Δsite 1.59e-4 Å

**`SYMMETRY_UNDETERMINED_BOTH`** — 1813 file(s)

- COD 2002141 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.03e-15 Å
- COD 2002142 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 6.22e-6 Å
- COD 2002153 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.05e-15 Å
- COD 2002188 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.74e-15 Å
- COD 2002212 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.70e-15 Å
- COD 2002220 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 5.18e-15 Å
- COD 2002226 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.14e-15 Å
- COD 2002234 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.81e-15 Å
- COD 2002274 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.22e-15 Å
- COD 2002278 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.56e-15 Å
- COD 2002294 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 2002299 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.51e-15 Å
- COD 2002301 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.06e-15 Å
- COD 2002307 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 2002346 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 5.41e-15 Å
- COD 2002355 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.71e-15 Å
- COD 2002390 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.18e-15 Å
- COD 2002399 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.23e-15 Å
- COD 2002417 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 4.26e-15 Å
- COD 2002435 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.70e-15 Å
- COD 2002444 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.24e-15 Å
- COD 2002456 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.76e-15 Å
- COD 2002475 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.94e-15 Å
- COD 2002492 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.24e-15 Å
- COD 2002497 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- … and 1788 more (see per_file.csv)

### Round trip B

**`PARSE_DEGENERATE_ORIG`** — 1 file(s)

- COD 3000271 — sg →, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å — degenerate lattice (volume 0.000e0)

**`COUNT_MISMATCH`** — 2 file(s)

- COD 2108792 — sg 5→5, max Δlen 0.00e0 Å, max Δsite -1.00e0 Å
- COD 3000042 — sg 194→6, max Δlen 0.00e0 Å, max Δsite -1.00e0 Å

**`SYMMETRY_CHANGED`** — 1 file(s)

- COD 9017201 — sg 1→5, max Δlen 0.00e0 Å, max Δsite 5.31e-6 Å

**`GEOM_MISMATCH`** — 1 file(s)

- COD 2107333 — sg 8→8, max Δlen 0.00e0 Å, max Δsite 1.59e-4 Å

**`LOSS_OCCUPANCY_EXPECTED`** — 4946 file(s)

- COD 2000015 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 3.25e-15 Å
- COD 2000087 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 1.27e-15 Å
- COD 2000242 — sg 11→11, max Δlen 0.00e0 Å, max Δsite 3.18e-15 Å
- COD 2000432 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 6.09e-15 Å
- COD 2000488 — sg 166→166, max Δlen 0.00e0 Å, max Δsite 1.27e-5 Å
- COD 2000631 — sg 19→19, max Δlen 0.00e0 Å, max Δsite 5.28e-15 Å
- COD 2000767 — sg 1→1, max Δlen 0.00e0 Å, max Δsite 2.44e-15 Å
- COD 2000913 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 3.79e-15 Å
- COD 2001022 — sg 1→1, max Δlen 0.00e0 Å, max Δsite 5.74e-15 Å
- COD 2001259 — sg 4→4, max Δlen 0.00e0 Å, max Δsite 3.15e-15 Å
- COD 2001271 — sg 60→60, max Δlen 0.00e0 Å, max Δsite 4.38e-15 Å
- COD 2001290 — sg 61→61, max Δlen 0.00e0 Å, max Δsite 4.35e-15 Å
- COD 2001325 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 4.64e-15 Å
- COD 2001612 — sg 205→205, max Δlen 0.00e0 Å, max Δsite 6.88e-15 Å
- COD 2001759 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 5.90e-15 Å
- COD 2001995 — sg 15→15, max Δlen 0.00e0 Å, max Δsite 4.63e-15 Å
- COD 2002086 — sg 1→1, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 2002089 — sg 15→15, max Δlen 0.00e0 Å, max Δsite 6.29e-15 Å
- COD 2002141 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.03e-15 Å
- COD 2002142 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 6.22e-6 Å
- COD 2002153 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.05e-15 Å
- COD 2002159 — sg 62→62, max Δlen 0.00e0 Å, max Δsite 2.16e-15 Å
- COD 2002179 — sg 229→229, max Δlen 0.00e0 Å, max Δsite 2.03e-15 Å
- COD 2002188 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.74e-15 Å
- COD 2002212 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.70e-15 Å
- … and 4921 more (see per_file.csv)

**`LOSS_OXIDATION_EXPECTED`** — 423 file(s)

- COD 2000108 — sg 15→15, max Δlen 0.00e0 Å, max Δsite 3.29e-15 Å
- COD 2002122 — sg 221→221, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 2002129 — sg 64→64, max Δlen 0.00e0 Å, max Δsite 1.97e-15 Å
- COD 2002150 — sg 15→15, max Δlen 0.00e0 Å, max Δsite 1.54e-15 Å
- COD 2002165 — sg 135→135, max Δlen 0.00e0 Å, max Δsite 3.03e-15 Å
- COD 2002168 — sg 9→9, max Δlen 0.00e0 Å, max Δsite 5.60e-15 Å
- COD 2002182 — sg 33→33, max Δlen 0.00e0 Å, max Δsite 2.40e-15 Å
- COD 2002191 — sg 74→74, max Δlen 0.00e0 Å, max Δsite 7.76e-16 Å
- COD 2002204 — sg 59→59, max Δlen 0.00e0 Å, max Δsite 1.30e-15 Å
- COD 2002205 — sg 59→59, max Δlen 0.00e0 Å, max Δsite 2.59e-15 Å
- COD 2002206 — sg 33→33, max Δlen 0.00e0 Å, max Δsite 1.28e-15 Å
- COD 2002209 — sg 62→62, max Δlen 0.00e0 Å, max Δsite 1.92e-15 Å
- COD 2002215 — sg 141→141, max Δlen 0.00e0 Å, max Δsite 2.79e-16 Å
- COD 2002219 — sg 108→108, max Δlen 0.00e0 Å, max Δsite 6.56e-16 Å
- COD 2002227 — sg 71→71, max Δlen 0.00e0 Å, max Δsite 1.22e-15 Å
- COD 2002233 — sg 79→79, max Δlen 0.00e0 Å, max Δsite 1.95e-15 Å
- COD 2002260 — sg 62→62, max Δlen 0.00e0 Å, max Δsite 9.38e-16 Å
- COD 2002265 — sg 64→64, max Δlen 0.00e0 Å, max Δsite 3.48e-16 Å
- COD 2002285 — sg 12→12, max Δlen 0.00e0 Å, max Δsite 3.12e-15 Å
- COD 2002313 — sg 55→55, max Δlen 0.00e0 Å, max Δsite 1.29e-15 Å
- COD 2002315 — sg 4→4, max Δlen 0.00e0 Å, max Δsite 2.25e-15 Å
- COD 2002318 — sg 63→63, max Δlen 0.00e0 Å, max Δsite 1.12e-15 Å
- COD 2002319 — sg 63→63, max Δlen 0.00e0 Å, max Δsite 1.62e-15 Å
- COD 2002322 — sg 5→5, max Δlen 0.00e0 Å, max Δsite 2.06e-15 Å
- COD 2002326 — sg 33→33, max Δlen 0.00e0 Å, max Δsite 2.74e-15 Å
- … and 398 more (see per_file.csv)

**`SYMMETRY_UNDETERMINED_BOTH`** — 28 file(s)

- COD 2010260 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.41e-15 Å
- COD 2101366 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 2102008 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 5.44e-6 Å
- COD 2102009 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 5.41e-6 Å
- COD 2311722 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 5910132 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.39e-15 Å
- COD 5910161 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.32e-15 Å
- COD 5910169 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.98e-15 Å
- COD 5910171 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 7.91e-16 Å
- COD 5910172 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 4.27e-16 Å
- COD 5910173 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.72e-16 Å
- COD 5910246 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.43e-15 Å
- COD 5910252 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.63e-16 Å
- COD 5910258 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.61e-16 Å
- COD 5910259 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.05e-15 Å
- COD 5910263 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 6.94e-16 Å
- COD 5910289 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.66e-15 Å
- COD 5910299 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.40e-15 Å
- COD 5910304 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.44e-15 Å
- COD 5910328 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.63e-15 Å
- COD 5910341 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 9.64e-16 Å
- COD 5910346 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.41e-16 Å
- COD 5910349 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.64e-16 Å
- COD 9000897 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 4.12e-15 Å
- COD 9001903 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 5.13e-15 Å
- … and 3 more (see per_file.csv)

## Method notes

- Space group compared as `symmetry::analyze()` on the **parsed original** vs the **parsed final**, never against the CIF header. SYMPREC = 1e-4.
- Classification rule: a structure is *organic / metal-organic* if it contains both C and H; otherwise *inorganic*. Hydrated carbonates fall on the organic side.
- POSCAR carries neither occupancy nor oxidation state; loss of these in round trip B is format-inherent and reported as `LOSS_*_EXPECTED`, not as failure.
- Tolerances: cell length 1e-5 Å, angle 1e-4°, volume 1e-5 relative, site position 1e-4 Å, occupancy 1e-3.
- Each file runs in an isolated child process; panics, aborts and hangs are recorded rather than ending the run. Timings are the second of two passes (warm page cache).
