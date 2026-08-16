# CView corpus round-trip fidelity audit

Files audited: **1000** (seed 42, per-file timeout 60 s)

## Round trip A — CIF → CIF

### All files

| Outcome | Files | % |
|---|---:|---:|
| `SYMMETRY_UNDETERMINED_BOTH` | 98 | 9.8 |
| `PASS` | 902 | 90.2 |

### Inorganic only

| Outcome | Files | % |
|---|---:|---:|
| `SYMMETRY_UNDETERMINED_BOTH` | 90 | 34.9 |
| `PASS` | 168 | 65.1 |

### Organic / metal-organic only

| Outcome | Files | % |
|---|---:|---:|
| `SYMMETRY_UNDETERMINED_BOTH` | 8 | 1.1 |
| `PASS` | 734 | 98.9 |

## Round trip B — CIF → POSCAR → CIF

### All files

| Outcome | Files | % |
|---|---:|---:|
| `LOSS_OCCUPANCY_EXPECTED` | 265 | 26.5 |
| `LOSS_OXIDATION_EXPECTED` | 20 | 2.0 |
| `SYMMETRY_UNDETERMINED_BOTH` | 1 | 0.1 |
| `PASS` | 714 | 71.4 |

### Inorganic only

| Outcome | Files | % |
|---|---:|---:|
| `LOSS_OCCUPANCY_EXPECTED` | 116 | 45.0 |
| `LOSS_OXIDATION_EXPECTED` | 14 | 5.4 |
| `SYMMETRY_UNDETERMINED_BOTH` | 1 | 0.4 |
| `PASS` | 127 | 49.2 |

### Organic / metal-organic only

| Outcome | Files | % |
|---|---:|---:|
| `LOSS_OCCUPANCY_EXPECTED` | 149 | 20.1 |
| `LOSS_OXIDATION_EXPECTED` | 6 | 0.8 |
| `PASS` | 587 | 79.1 |

## Timing (single-file latency, warm cache, release build)

| Quantity | Median | p95 | Max |
|---|---:|---:|---:|
| `load_structure` parse (ms) | 0.27 | 1.38 | 13.89 |
| Round trip A total (ms) | 0.65 | 2.62 | 34.86 |
| Round trip B total (ms) | 1.24 | 4.66 | 58.39 |

Worst-case parse: COD **2019361** — 13.9 ms, 56093 bytes, 5664 atoms, reflection block: true.

## Notable failures

### Round trip A

**`SYMMETRY_UNDETERMINED_BOTH`** — 98 file(s)

- COD 2002912 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.39e-15 Å
- COD 2005854 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.84e-15 Å
- COD 2008182 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.30e-15 Å
- COD 2015334 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.98e-15 Å
- COD 2015884 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 4.48e-15 Å
- COD 2020885 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.14e-15 Å
- COD 2021683 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 4.44e-15 Å
- COD 2104529 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 7.72e-15 Å
- COD 2106060 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.05e-14 Å
- COD 2106635 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.86e-15 Å
- COD 2107227 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.70e-15 Å
- COD 2107283 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.42e-16 Å
- COD 2107452 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 2107993 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.73e-15 Å
- COD 2233423 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.55e-15 Å
- COD 2300615 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.61e-15 Å
- COD 2310505 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 4.27e-15 Å
- COD 2311708 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 5.47e-6 Å
- COD 2311722 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 2312629 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.32e-15 Å
- COD 3000125 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 6.25e-16 Å
- COD 7006080 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.74e-15 Å
- COD 9000166 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.14e-15 Å
- COD 9000758 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.84e-15 Å
- COD 9001163 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.04e-15 Å
- … and 73 more (see per_file.csv)

### Round trip B

**`LOSS_OCCUPANCY_EXPECTED`** — 265 file(s)

- COD 2001259 — sg 4→4, max Δlen 0.00e0 Å, max Δsite 3.15e-15 Å
- COD 2002369 — sg 38→38, max Δlen 0.00e0 Å, max Δsite 2.26e-15 Å
- COD 2002563 — sg 221→221, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 2002912 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.39e-15 Å
- COD 2002995 — sg 54→54, max Δlen 0.00e0 Å, max Δsite 4.45e-15 Å
- COD 2003215 — sg 113→113, max Δlen 0.00e0 Å, max Δsite 2.93e-15 Å
- COD 2003894 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 5.37e-15 Å
- COD 2004121 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 3.75e-15 Å
- COD 2004575 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 1.19e-14 Å
- COD 2004821 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 4.13e-15 Å
- COD 2005389 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 2.50e-15 Å
- COD 2005854 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 3.84e-15 Å
- COD 2006430 — sg 5→5, max Δlen 0.00e0 Å, max Δsite 3.50e-15 Å
- COD 2006507 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 2.33e-15 Å
- COD 2006753 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 5.01e-15 Å
- COD 2008182 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 2.30e-15 Å
- COD 2010434 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 2.93e-15 Å
- COD 2010554 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 2.99e-15 Å
- COD 2012285 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 3.82e-15 Å
- COD 2013381 — sg 13→13, max Δlen 0.00e0 Å, max Δsite 2.49e-15 Å
- COD 2013997 — sg 19→19, max Δlen 0.00e0 Å, max Δsite 5.89e-15 Å
- COD 2015334 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 1.98e-15 Å
- COD 2015884 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 4.48e-15 Å
- COD 2016022 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 3.45e-15 Å
- COD 2016659 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 3.99e-15 Å
- … and 240 more (see per_file.csv)

**`LOSS_OXIDATION_EXPECTED`** — 20 file(s)

- COD 2002394 — sg 127→127, max Δlen 0.00e0 Å, max Δsite 7.30e-16 Å
- COD 2002722 — sg 15→15, max Δlen 0.00e0 Å, max Δsite 3.01e-15 Å
- COD 2006468 — sg 33→33, max Δlen 0.00e0 Å, max Δsite 2.84e-15 Å
- COD 2020200 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 4.99e-15 Å
- COD 2105392 — sg 220→220, max Δlen 0.00e0 Å, max Δsite 1.45e-15 Å
- COD 2106044 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 2.43e-15 Å
- COD 2106576 — sg 15→15, max Δlen 0.00e0 Å, max Δsite 1.15e-15 Å
- COD 2106722 — sg 15→15, max Δlen 0.00e0 Å, max Δsite 3.25e-15 Å
- COD 2106816 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 1.10e-15 Å
- COD 2106915 — sg 9→9, max Δlen 0.00e0 Å, max Δsite 4.71e-16 Å
- COD 2107037 — sg 62→62, max Δlen 0.00e0 Å, max Δsite 1.55e-15 Å
- COD 2107058 — sg 225→225, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 2108891 — sg 40→40, max Δlen 0.00e0 Å, max Δsite 5.42e-6 Å
- COD 2300544 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 1.13e-15 Å
- COD 2310419 — sg 2→2, max Δlen 0.00e0 Å, max Δsite 1.71e-15 Å
- COD 2310921 — sg 25→25, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å
- COD 7000051 — sg 61→61, max Δlen 0.00e0 Å, max Δsite 3.97e-15 Å
- COD 7009510 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 5.55e-15 Å
- COD 7009563 — sg 14→14, max Δlen 0.00e0 Å, max Δsite 9.63e-15 Å
- COD 7009614 — sg 4→4, max Δlen 0.00e0 Å, max Δsite 3.48e-15 Å

**`SYMMETRY_UNDETERMINED_BOTH`** — 1 file(s)

- COD 2311722 — sg NA→NA, max Δlen 0.00e0 Å, max Δsite 0.00e0 Å

## Method notes

- Space group compared as `symmetry::analyze()` on the **parsed original** vs the **parsed final**, never against the CIF header. SYMPREC = 1e-4.
- Classification rule: a structure is *organic / metal-organic* if it contains both C and H; otherwise *inorganic*. Hydrated carbonates fall on the organic side.
- POSCAR carries neither occupancy nor oxidation state; loss of these in round trip B is format-inherent and reported as `LOSS_*_EXPECTED`, not as failure.
- Tolerances: cell length 1e-5 Å, angle 1e-4°, volume 1e-5 relative, site position 1e-4 Å, occupancy 1e-3.
- Each file runs in an isolated child process; panics, aborts and hangs are recorded rather than ending the run. Timings are the second of two passes (warm page cache).
