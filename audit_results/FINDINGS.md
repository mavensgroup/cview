# CView corpus round-trip fidelity audit — findings

Supporting Information for the CView manuscript (J. Appl. Cryst.).

This is a **correctness audit**, not a throughput benchmark. It asks whether
CView's parser/writer stack preserves a crystal structure across a round trip
over the diversity of real crystallographic files. Timing is a secondary
by-product, reported as single-file latency — the figure of merit for an
interactive tool — never as throughput.

---

## 1. Corpus and its limitations (read this first)

The intended frame was the complete Crystallography Open Database archive
`cod-cifs-mysql.txz` (release 2025.02.09, SVN r297631, 18.5 GB). **The download
was deliberately stopped at 5.25 GB of 18.5 GB**, so the audit ran against the
portion of the archive that had been extracted at that point.

| Property | Value |
|---|---|
| Files in sampling frame | **111,270** |
| Fraction of full COD | roughly 20 % |
| Archive release | 2025.02.09 (SVN r297631) |
| Excluded | 1 file (see `excluded_files.txt`) |

**The frame is not a uniform random sample of COD.** Because tar members were
read in archive order, coverage is concentrated in particular COD ID ranges:

| COD ID range | Files | Note |
|---|---:|---|
| `2xxxxxx` | 80,212 | COD depositions, journal-derived |
| `9xxxxxx` | 17,926 | Mineral data (American Mineralogist etc.) |
| `7xxxxxx` | 10,811 | |
| `5xxxxxx` | 810 | |
| `6xxxxxx` | 785 | |
| `3xxxxxx` | 726 | |
| `1xxxxxx` | **0** | **entirely absent** |

The `1xxxxxx` range — a large block of journal depositions — is missing
altogether. Any statement in the paper must therefore be scoped as *"a sample of
111,270 COD entries"*, **not** *"a sample of COD"*. Sampling **within** this
frame is uniform, seeded and reproducible (`--seed 42`, xoshiro256\*\* through
SplitMix64, vendored in the harness so no dependency version can change it).

**One exclusion**, per the rule that failures are data and only non-structures
may be excluded: COD `7133683` was truncated mid-line by the interrupted archive
extraction. It is an artefact of the extraction, not a real COD file, and is
recorded in `excluded_files.txt`.

COD `1556838` (chibaite) is absent from the frame because it lies in the missing
`1xxxxxx` range; it was fetched directly from COD and audited separately
(§5).

---

## 2. What was run

| Stratum | n | Purpose |
|---|---:|---|
| **Primary** (`primary/`) | 1,000 | Headline number, seed 42 |
| **Extended** (`extended/`) | 20,000 | Tighter statistics; finds rare defects |
| **Supplement** (`supplement/`) | 78 | Targeted top-up of under-represented categories |

### Category coverage

Every category required of the sample reaches n ≥ 20. The primary sample alone
covered all but three crystal systems; the supplement was drawn specifically to
top those up, and is reported as a **separate stratum** so it cannot bias the
random-sample statistics in §3.

| Category | Primary | + Supplement | ≥ 20 |
|---|---:|---:|:--:|
| Partial site occupancy | 265 | — | ✔ |
| Split sites (coincident species) | 102 | — | ✔ |
| Embedded `_refln_*` block | 748 | — | ✔ |
| Multi-line semicolon text field | 999 | — | ✔ |
| Explicit oxidation states | 28 | — | ✔ |
| Non-standard setting (declared ≠ detected) | 20 | +20 | ✔ |
| Triclinic | 242 | 249 | ✔ |
| Monoclinic | 417 | 424 | ✔ |
| Orthorhombic | 154 | 157 | ✔ |
| Tetragonal | 23 | 44 | ✔ |
| Trigonal | 21 | 41 | ✔ |
| Hexagonal | **12** | **32** | ✔ |
| Cubic | 33 | 33 | ✔ |

"Non-standard setting" uses the reproducible proxy *declared space-group number
differs from the number detected from the coordinates*; that set is a superset
of genuine non-standard settings, since it also catches incorrect headers.

Both round trips run on every file:

- **A — CIF → CIF.** Parser + writer fidelity within a format that can carry
  everything.
- **B — CIF → POSCAR → CIF.** The actual DFT-preparation workflow.

Comparison is always **first parsed structure vs final parsed structure**, both
through the same parser. The space group is compared as
`symmetry::analyze()` on both sides — **never** against the CIF header, which
would measure COD's metadata quality rather than CView's fidelity.

Tolerances are fixed by writer precision, stated before the run and not
adjusted afterwards: cell length 1e-5 Å, angle 1e-4°, volume 1e-5 relative,
site position 1e-4 Å (PBC-aware, order-independent), occupancy 1e-3.
SYMPREC is the crate constant, 1e-4.

---

## 3. Headline results

### Round trip A — CIF → CIF

| Stratum | n | PASS | Symmetry undetermined (both sides) | Real defects |
|---|---:|---:|---:|---:|
| All | 1,000 | **90.2 %** | 98 | **0** |
| Inorganic | 258 | 65.1 % | 90 | 0 |
| Organic / metal-organic | 742 | 98.9 % | 8 | 0 |

### Round trip B — CIF → POSCAR → CIF

| Stratum | n | PASS | Occupancy lost | Oxidation lost | Real defects |
|---|---:|---:|---:|---:|---:|
| All | 1,000 | **71.4 %** | 265 | 20 | **0** |
| Inorganic | 258 | 49.2 % | 116 | 14 | 0 |
| Organic / metal-organic | 742 | 79.1 % | 149 | 6 | 0 |

**The PASS percentages badly understate fidelity and must not be quoted bare.**
Every single non-PASS outcome in the 1,000-file primary sample is one of two
things that are not parser defects:

1. **`LOSS_OCCUPANCY_EXPECTED` / `LOSS_OXIDATION_EXPECTED`** (round trip B only).
   POSCAR has no field for site occupancy or oxidation state. This loss is
   format-inherent, occurs by construction, and CView warns on export. It
   accounts for the entire A→B drop from 90.2 % to 71.4 %.
2. **`SYMMETRY_UNDETERMINED_BOTH`.** `moyo` returned an error on *both* sides,
   so the round trip changed nothing. See §6.

**Correctly stated: in the 1,000-file primary sample, zero files suffered any
loss of structural information that the target format could have carried.**

The inorganic/organic split is reported because CView targets extended
inorganic and metallic systems while COD is dominated by organics — but nothing
was filtered. Classification rule: a structure is *organic or metal-organic* if
it contains **both C and H**, otherwise *inorganic*. Simple and reproducible;
known edge case — hydrated carbonates fall on the organic side.

---

## 4. Real defects (extended run, 20,000 files)

The primary sample contained none. Scaling to 20,000 found **5 files, 0.025 %**.
Each was diagnosed by hand.

| COD ID | Outcome | Diagnosis |
|---|---|---|
| `3000042` | `COUNT_MISMATCH` + `SYMMETRY_CHANGED` (194→6) | Symmetry-expansion dedup tolerance — see below |
| `2108792` | `COUNT_MISMATCH` | Same cause |
| `9017201` | `SYMMETRY_CHANGED` (1→5) | Symmetry *gained* after coordinate rounding |
| `2107333` | `GEOM_MISMATCH` | CIF writer coordinate precision on a 476 Å axis |
| `3000271` | `PARSE_DEGENERATE_ORIG` | Source entry has no usable cell (volume 0) |

### 4.1 The dedup tolerance bug — the one that matters

`io/cif.rs` deduplicates symmetry-generated images using

```rust
let epsilon = 1e-3;                     // FRACTIONAL units, per-axis
let coincident = (dx < epsilon || (1.0 - dx) < epsilon) && /* … y, z … */;
```

Two problems:

1. **The tolerance is fractional, not metric.** 0.001 fractional is 0.006 Å in a
   6 Å cell but 0.48 Å in a 476 Å cell. Its physical meaning changes with the
   structure.
2. **It sits exactly on a very common data-precision boundary.** Coordinates
   deposited to 3 decimal places are ubiquitous in older CIFs. In COD `3000042`,
   F is deposited at `0.140(2), 0.281(2)`; symmetry mates that *should* be
   exactly coincident instead land 0.001 fractional (0.0063 Å) apart. The test
   `dx < 0.001` is then decided by floating-point noise in the last bit — the
   expansion path and the re-read path fall on opposite sides of it.

Observable consequence: loading `3000042` gives **46 atoms**; exporting to CIF
and reloading gives **44**, and the detected space group drops from
**P6₃/mmc (194) to C2 (6)**. In the GUI this is a silent, user-visible
corruption.

**Recommended fix** — convert the test to a Cartesian distance tolerance
(build the lattice before the expansion loop and compare real distances,
~0.05 Å), rather than a per-axis fractional one. I deliberately did **not**
make this change: it alters core CIF parsing for every file and could shift
atom counts feeding XRD and BVS results elsewhere in the manuscript. That is
your call, not mine.

### 4.2 Writer coordinate precision

COD `2107333` has c = **475.98 Å**. The CIF writer emits fractional coordinates
as `{:.6}`, so one ulp is 4.8e-4 Å on that axis — larger than the 1e-4 Å
position tolerance. Observed deviation 1.59e-4 Å. This is a genuine precision
limit, not corruption. **Cheap fix with no downside: widen the CIF writer's
fractional coordinate format from `{:.6}` to `{:.9}`**, matching what the POSCAR
writer already does. Worth doing before publication.

### 4.3 Symmetry gained on round trip

COD `9017201`: the parsed original is detected as P1 (1), the round-tripped
structure as C2 (5), with a maximum site deviation of only 5.3e-6 Å. Rounding
to the writer's 6 decimal places pushed near-symmetric coordinates *inside*
SYMPREC. This is a tolerance-boundary effect rather than data loss, and is
the same phenomenon quantified in §7.

---

## 5. Panics, timeouts, and the worst case

**Zero panics. Zero timeouts. Zero crashes.** Across 21,078 audited files
(1,000 + 20,000 + 78) plus the shakeout runs, no file panicked, aborted,
overflowed the stack, or exceeded the 60 s per-file wall-clock limit.

This is a strong claim because of how it was measured: each file is processed
in an **isolated child process**, so a panic, an abort, a stack overflow or an
infinite loop is caught and recorded rather than ending the run.
`catch_unwind` alone could not have caught the last three.

**Worst case by parse time in the corpus:** COD `2311717` — 103 kB, 10,240
atoms after symmetry expansion, **47.4 ms**. The cost is atom-count-driven, not
file-size-driven.

**COD 1556838 (chibaite)**, the pathological case cited in the manuscript —
4.1 MB, 125,442 lines dominated by an embedded reflection block:

| | Result |
|---|---|
| Parse | **4.22 ms** |
| Round trip A | **PASS**, space group 15 → 15 |
| Round trip B | `LOSS_OCCUPANCY_EXPECTED` (format-inherent) |

4 ms for a 4 MB file confirms the multi-line semicolon text-field skip is
working: the reflection block is stepped over rather than tokenised.

---

## 6. Symmetry undetermined — not a defect, but worth reporting

In the primary sample 98 files (9.8 %) returned `SYMMETRY_UNDETERMINED_BOTH`,
rising to 90 of 258 (34.9 %) among inorganics. In **every** case `moyo`
failed on both the original and the round-tripped structure, so round-trip
fidelity is unaffected — which is why this is graded separately from
`SYMMETRY_LOST` / `SYMMETRY_GAINED` (a one-sided failure, which would be
serious). **No one-sided symmetry failure occurred in any run.**

The cause is disorder: these structures carry split sites — two species at
coincident positions — and a symmetry search cannot handle coincident atoms.
The taxonomy was split after the smoke run showed this pattern, rather than
being assumed up front.

**This is still a real usability finding for the paper:** CView's Symmetry
panel will report a failed search for roughly a third of inorganic COD entries,
because they are disordered. That deserves a sentence in the manuscript, and
ideally a clearer message in the UI than a bare failure.

---

## 7. Tolerance sensitivity (SYMPREC)

The detected space group of the parsed original was recomputed at symprec
1e-3, 1e-4 and 1e-5:

| Stratum | Identical at all three | Varies |
|---|---:|---:|
| Primary (1,000) | 977 | **23 (2.3 %)** |
| Extended (20,000) | 19,649 | **350 (1.75 %)** |

So for roughly 2 % of real files the reported space group depends on the
tolerance. The variation is overwhelmingly concentrated in trigonal and
hexagonal groups, and is almost always a *drop* to lower symmetry at the
tighter 1e-5 (e.g. `194 / 194 / 63` — P6₃/mmc at 1e-3 and 1e-4 becoming Cmcm at
1e-5; `164 / 164 / 5`; `176 / 1 / 1`).

The crate's single shared `SYMPREC = 1e-4` sits in the stable middle of this
range, which supports the existing design decision to use one constant
everywhere. But the manuscript should state that space-group assignment near
this tolerance is not unconditional — about 2 % of entries are tolerance-
sensitive.

---

## 8. Declared vs detected space group (context on COD, not on CView)

Recorded but never used for pass/fail, since comparing against the CIF header
would measure database metadata quality:

| Stratum | Header agrees | Header differs | Not comparable |
|---|---:|---:|---:|
| Primary (1,000) | 854 | 20 | 126 |
| Extended (20,000) | 17,231 | 388 | 2,381 |

About 2 % of entries declare a space-group number that differs from the one
detected from the coordinates — non-standard settings and incorrect headers
combined. "Not comparable" means the header lacked a number, or the search
failed (§6).

---

## 9. Timing

Single-file latency. Release build, warm page cache — every file is processed
twice and the first pass discarded, so these are not disk numbers. Median and
p95 are reported rather than the mean, because the distribution has a heavy
tail that a mean would misrepresent.

**Primary sample (n = 1,000):**

| Quantity | Median | p95 | Max |
|---|---:|---:|---:|
| `load_structure` parse | **0.27 ms** | 1.38 ms | 13.9 ms |
| Round trip A total | 0.65 ms | 2.62 ms | 34.9 ms |
| Round trip B total | 1.24 ms | 4.66 ms | 58.4 ms |

**Extended sample (n = 20,000):**

| Quantity | Median | p95 | Max |
|---|---:|---:|---:|
| `load_structure` parse | **0.26 ms** | 1.27 ms | 47.4 ms |
| Round trip A total | 0.65 ms | 2.35 ms | 81.6 ms |
| Round trip B total | 1.23 ms | 4.42 ms | 122.2 ms |

Median source file is 14.0 kB. Correlation between file size and parse time is
moderate (Pearson r = 0.71): size alone does not predict cost, because parse
time tracks the atom count *after* symmetry expansion, and because large
embedded reflection blocks are skipped rather than parsed.

**Measurement machine:**

| | |
|---|---|
| CPU | Intel Core Ultra 5 125H (18 logical cores) |
| Storage | NVMe SSD |
| OS | Fedora Linux 44 (Workstation Edition) |
| rustc | 1.94.0 (4a4ef493e 2026-03-02) |
| Build | `--release` (opt-level 3, LTO, codegen-units 1) |
| CView | 1.0.0 |

Only one core is used: files are processed sequentially so that timings are not
distorted by contention.

---

## 10. Two parser behaviours worth knowing about

**`cif::parse` has no error return path.** It fails only if the file cannot be
opened. Fed plain prose, an empty file, or a header-only file, it returns
`Ok` with a zero-atom structure. In the GUI this is a silent "successful" load
of nothing. The audit detects it as `PARSE_DEGENERATE_ORIG` (1 occurrence in
20,000 — COD `3000271`, an entry with no usable cell) rather than letting it
pass. Whether the parser should reject malformed input is a behavioural change
with GUI consequences and is left as your decision.

**The POSCAR fallback hazard does not apply to `.cif`.** `io::load_structure`
routes any `.cif` path straight to `cif::parse` and never falls through to the
POSCAR parser, so a failed CIF parse cannot be silently rescued into a garbage
structure by the fallback. The hazard is real for extensionless files only.
The related risk is the one above: a bad CIF does not fail, it returns empty.

---

## 11. Fixes already made (and re-run against)

Both were found by this audit and are covered by a new regression test
(`write_then_parse_preserves_oxidation_and_occupancy`). All results above were
produced **after** these fixes.

1. **Oxidation state was lost even in round trip A.** The CIF writer emitted
   only `_atom_site_label` and never `_atom_site_type_symbol`, so the formal
   charge read from the source file was silently dropped on export and BVS fell
   back to a guessed valence on re-import. The writer now emits
   `_atom_site_type_symbol` with the charge (`Fe3+`, `O2-`). This was a genuine
   defect, not format-inherent loss.

2. **Explicit neutral states were downgraded to unknown.** Fixing (1) exposed a
   second case: CIFs declaring `_atom_type_oxidation_number 0` parse to
   `Some(0)`, which the first fix wrote as a bare symbol and re-read as `None`.
   "Known to be neutral" silently became "unknown". Now written as `Co0+`, the
   ICSD convention, which the existing parser already round-trips.

---

## 12. Recommendations, in priority order

1. **Fix the CIF dedup tolerance (§4.1).** A real, user-visible corruption:
   atom count and space group both change on export/reload. Rare (2 in 20,000)
   but it silently corrupts data in the GUI. Use a Cartesian distance tolerance
   instead of a per-axis fractional one. **Highest priority; needs your
   judgement because it touches every CIF load.**
2. **Widen CIF writer coordinate precision from `{:.6}` to `{:.9}` (§4.2).**
   Trivial, no downside, removes the only geometry failure observed.
3. **Improve the Symmetry panel message for disordered structures (§6).**
   A third of inorganic COD entries will show a failed symmetry search; a
   message explaining that split-site disorder prevents the search would be
   far better than a bare failure.
4. **Consider making `cif::parse` reject unparseable input (§10).** Behavioural
   change; your call.
5. **State the corpus limitation honestly in the SI (§1).** The frame is ~20 %
   of COD with the `1xxxxxx` range absent. A referee who checks will find this
   immediately; owning it costs nothing, and the reproducible manifest lets
   them verify the sample.

---

## 13. Files in this directory

| File | Contents |
|---|---|
| `primary/per_file.csv` | One row per file per round trip, n = 1,000 |
| `primary/summary.json` | Aggregates, timing percentiles, machine metadata |
| `primary/summary.md` | Generated tables |
| `primary/corpus_manifest.txt` | Exact COD IDs used — reproducible |
| `extended/*` | Same, n = 20,000 (`per_file.csv.gz`) |
| `supplement/*` | Same, n = 78 targeted category top-up |
| `excluded_files.txt` | Every exclusion, with reason |

Reproduce with:

```
cargo run --release --features audit --bin corpus_audit -- \
    --corpus <cod-cif-tree> --sample 1000 --seed 42 --out audit_results/primary
```

All output is sorted by COD ID so successive runs diff cleanly.
