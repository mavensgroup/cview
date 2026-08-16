# CView corpus round-trip fidelity audit

Files audited: **78** (seed 42, per-file timeout 60 s)

## Round trip A — CIF → CIF

### All files

| Outcome | Files | % |
|---|---:|---:|
| `PASS` | 78 | 100.0 |

### Inorganic only

| Outcome | Files | % |
|---|---:|---:|
| `PASS` | 50 | 100.0 |

### Organic / metal-organic only

| Outcome | Files | % |
|---|---:|---:|
| `PASS` | 28 | 100.0 |

## Round trip B — CIF → POSCAR → CIF

### All files

| Outcome | Files | % |
|---|---:|---:|
| `LOSS_OCCUPANCY_EXPECTED` | 11 | 14.1 |
| `LOSS_OXIDATION_EXPECTED` | 21 | 26.9 |
| `PASS` | 46 | 59.0 |

### Inorganic only

| Outcome | Files | % |
|---|---:|---:|
| `LOSS_OCCUPANCY_EXPECTED` | 9 | 18.0 |
| `LOSS_OXIDATION_EXPECTED` | 21 | 42.0 |
| `PASS` | 20 | 40.0 |

### Organic / metal-organic only

| Outcome | Files | % |
|---|---:|---:|
| `LOSS_OCCUPANCY_EXPECTED` | 2 | 7.1 |
| `PASS` | 26 | 92.9 |

## Timing (single-file latency, warm cache, release build)

| Quantity | Median | p95 | Max |
|---|---:|---:|---:|
| `load_structure` parse (ms) | 0.12 | 0.97 | 4.88 |
| Round trip A total (ms) | 0.32 | 2.78 | 4.50 |
| Round trip B total (ms) | 0.63 | 5.44 | 8.41 |

Worst-case parse: COD **2023290** — 4.9 ms, 310970 bytes, 652 atoms, reflection block: true.

## Notable failures

### Round trip B

**`LOSS_OCCUPANCY_EXPECTED`** — 11 file(s)

- COD 2000488 — sg 166→166, max Δlen 0.00e0 Å, max Δsite 1.27e-5 Å
- COD 2002286 — sg 63→63, max Δlen 0.00e0 Å, max Δsite 7.88e-16 Å
- COD 2002387 — sg 11→11, max Δlen 0.00e0 Å, max Δsite 2.27e-15 Å
- COD 2002405 — sg 176→176, max Δlen 0.00e0 Å, max Δsite 2.21e-15 Å
- COD 2002755 — sg 36→36, max Δlen 0.00e0 Å, max Δsite 1.60e-15 Å
- COD 2002849 — sg 63→63, max Δlen 0.00e0 Å, max Δsite 1.18e-15 Å
- COD 2003159 — sg 166→166, max Δlen 0.00e0 Å, max Δsite 1.01e-5 Å
- COD 2006776 — sg 164→164, max Δlen 0.00e0 Å, max Δsite 2.00e-16 Å
- COD 2015632 — sg 169→169, max Δlen 0.00e0 Å, max Δsite 3.29e-6 Å
- COD 2017834 — sg 170→170, max Δlen 0.00e0 Å, max Δsite 7.37e-6 Å
- COD 2023290 — sg 173→173, max Δlen 0.00e0 Å, max Δsite 1.28e-14 Å

**`LOSS_OXIDATION_EXPECTED`** — 21 file(s)

- COD 2002165 — sg 135→135, max Δlen 0.00e0 Å, max Δsite 3.03e-15 Å
- COD 2002168 — sg 9→9, max Δlen 0.00e0 Å, max Δsite 5.60e-15 Å
- COD 2002215 — sg 141→141, max Δlen 0.00e0 Å, max Δsite 2.79e-16 Å
- COD 2002219 — sg 108→108, max Δlen 0.00e0 Å, max Δsite 6.56e-16 Å
- COD 2002233 — sg 79→79, max Δlen 0.00e0 Å, max Δsite 1.95e-15 Å
- COD 2002315 — sg 4→4, max Δlen 0.00e0 Å, max Δsite 2.25e-15 Å
- COD 2002354 — sg 142→142, max Δlen 0.00e0 Å, max Δsite 1.38e-15 Å
- COD 2002370 — sg 127→127, max Δlen 0.00e0 Å, max Δsite 1.12e-15 Å
- COD 2002371 — sg 127→127, max Δlen 0.00e0 Å, max Δsite 1.11e-15 Å
- COD 2002418 — sg 139→139, max Δlen 0.00e0 Å, max Δsite 6.02e-16 Å
- COD 2002422 — sg 15→15, max Δlen 0.00e0 Å, max Δsite 3.12e-15 Å
- COD 2002433 — sg 136→136, max Δlen 0.00e0 Å, max Δsite 7.41e-16 Å
- COD 2002466 — sg 139→139, max Δlen 0.00e0 Å, max Δsite 1.14e-15 Å
- COD 2002504 — sg 127→127, max Δlen 0.00e0 Å, max Δsite 9.58e-16 Å
- COD 2002509 — sg 140→140, max Δlen 0.00e0 Å, max Δsite 7.52e-16 Å
- COD 2002556 — sg 87→87, max Δlen 0.00e0 Å, max Δsite 3.13e-15 Å
- COD 2002576 — sg 145→145, max Δlen 0.00e0 Å, max Δsite 6.33e-6 Å
- COD 2002599 — sg 146→146, max Δlen 0.00e0 Å, max Δsite 9.14e-6 Å
- COD 2002739 — sg 11→11, max Δlen 0.00e0 Å, max Δsite 1.63e-15 Å
- COD 2002756 — sg 167→167, max Δlen 0.00e0 Å, max Δsite 6.77e-6 Å
- COD 2002852 — sg 1→1, max Δlen 0.00e0 Å, max Δsite 1.41e-15 Å

## Method notes

- Space group compared as `symmetry::analyze()` on the **parsed original** vs the **parsed final**, never against the CIF header. SYMPREC = 1e-4.
- Classification rule: a structure is *organic / metal-organic* if it contains both C and H; otherwise *inorganic*. Hydrated carbonates fall on the organic side.
- POSCAR carries neither occupancy nor oxidation state; loss of these in round trip B is format-inherent and reported as `LOSS_*_EXPECTED`, not as failure.
- Tolerances: cell length 1e-5 Å, angle 1e-4°, volume 1e-5 relative, site position 1e-4 Å, occupancy 1e-3.
- Each file runs in an isolated child process; panics, aborts and hangs are recorded rather than ending the run. Timings are the second of two passes (warm page cache).
