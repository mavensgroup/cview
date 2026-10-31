// src/model/bonds.rs
//
// Which atom pairs are bonded: one rule per element pair (minimum and
// maximum length), used by the main view, the polyhedra, the coordination
// numbers and the trajectory player alike.
//
// Defaults come from VESTA's bond list (see `vesta_bonds.rs`), so a
// structure shows the bonds VESTA users expect: Ti–O and Ba–O in BaTiO₃ but
// no Ba–Ti, Fe–O in an iron oxide but no Fe–Fe. VESTA lists no metal–metal
// pairs and leaves metals and intermetallics without bonds; CView instead
// bonds elements that have no listed partner in the structure by covalent
// radii × tolerance (what it did for every pair before), so a Heusler alloy
// still shows its bonds.

use crate::model::elements::get_atom_cov;
use crate::model::vesta_bonds::VESTA_BONDS;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

/// Atoms closer than this (Å) are never bonded: they are overlapping or
/// split sites, not neighbours.
pub const MIN_SEPARATION: f64 = 0.4;

/// Where a rule's lengths come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleSource {
    /// VESTA's default bond list.
    Vesta,
    /// Covalent radii × tolerance (elements VESTA gives no partner here).
    Covalent,
    /// No default: off until switched on (the length is a suggestion).
    None,
}

impl RuleSource {
    pub fn name(self) -> &'static str {
        match self {
            RuleSource::Vesta => "VESTA",
            RuleSource::Covalent => "covalent",
            RuleSource::None => "—",
        }
    }
}

/// A bond rule for one element pair; `a` ≤ `b` alphabetically.
#[derive(Clone, Debug, PartialEq)]
pub struct BondRule {
    pub a: String,
    pub b: String,
    pub min: f64,
    pub max: f64,
    pub enabled: bool,
    /// Where the default came from (kept when the user edits the rule).
    pub source: RuleSource,
}

/// The key of an element pair, order-independent.
pub fn pair_key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// VESTA's maximum bond length for a pair, if it lists one. Where VESTA
/// gives two directional entries (O→H 1.2 Å covalent, H→O 2.1 Å hydrogen
/// bond) the shorter, covalent one is used.
pub fn vesta_max(a: &str, b: &str) -> Option<f64> {
    static TABLE: OnceLock<HashMap<(String, String), f64>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut m: HashMap<(String, String), f64> = HashMap::new();
        for (x, y, d) in VESTA_BONDS.iter() {
            let e = m.entry(pair_key(x, y)).or_insert(*d);
            *e = e.min(*d);
        }
        m
    });
    table.get(&pair_key(a, b)).copied()
}

/// Covalent-radius bond length × `tolerance`, the rule for pairs VESTA
/// does not cover.
pub fn covalent_max(a: &str, b: &str, tolerance: f64) -> f64 {
    (get_atom_cov(a) + get_atom_cov(b)) * tolerance
}

/// Default rules for every pair of `elements` (any order, duplicates fine).
pub fn default_rules(elements: &[String], tolerance: f64) -> Vec<BondRule> {
    let mut els: Vec<&str> = elements.iter().map(String::as_str).collect();
    els.sort_unstable();
    els.dedup();

    // Elements with at least one VESTA-listed partner in this structure.
    let partnered: Vec<bool> = els
        .iter()
        .map(|a| els.iter().any(|b| vesta_max(a, b).is_some()))
        .collect();

    let mut rules = Vec::new();
    for i in 0..els.len() {
        for j in i..els.len() {
            let (a, b) = (els[i], els[j]);
            let (max, enabled, source) = match vesta_max(a, b) {
                Some(d) => (d, true, RuleSource::Vesta),
                None if !partnered[i] && !partnered[j] => {
                    (covalent_max(a, b, tolerance), true, RuleSource::Covalent)
                }
                None => (covalent_max(a, b, tolerance), false, RuleSource::None),
            };
            rules.push(BondRule {
                a: a.to_string(),
                b: b.to_string(),
                min: 0.0,
                max,
                enabled,
                source,
            });
        }
    }
    rules
}

/// Fast lookup of the enabled rules, for the hot bond-search loops.
#[derive(Clone, Debug, Default)]
pub struct BondTable {
    index: HashMap<String, usize>,
    n: usize,
    range: Vec<Option<(f64, f64)>>,
    max: f64,
    fingerprint: u64,
}

impl BondTable {
    pub fn from_rules(rules: &[BondRule]) -> Self {
        let mut index: HashMap<String, usize> = HashMap::new();
        for r in rules {
            for e in [&r.a, &r.b] {
                let next = index.len();
                index.entry(e.clone()).or_insert(next);
            }
        }
        let n = index.len();
        let mut range = vec![None; n * n];
        let mut max: f64 = 0.0;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for r in rules.iter().filter(|r| r.enabled && r.max > r.min) {
            let (i, j) = (index[&r.a], index[&r.b]);
            range[i * n + j] = Some((r.min, r.max));
            range[j * n + i] = Some((r.min, r.max));
            max = max.max(r.max);
            pair_key(&r.a, &r.b).hash(&mut h);
            r.min.to_bits().hash(&mut h);
            r.max.to_bits().hash(&mut h);
        }
        Self { index, n, range, max, fingerprint: h.finish() }
    }

    /// The (min, max) length of an enabled rule for the pair.
    pub fn range(&self, a: &str, b: &str) -> Option<(f64, f64)> {
        let (i, j) = (*self.index.get(a)?, *self.index.get(b)?);
        self.range[i * self.n + j]
    }

    /// Whether atoms of elements `a` and `b` at distance `d` are bonded.
    pub fn bonded(&self, a: &str, b: &str, d: f64) -> bool {
        matches!(self.range(a, b), Some((lo, hi)) if d >= MIN_SEPARATION && d > lo && d <= hi)
    }

    /// The longest bond any rule allows (Å); 0 when nothing bonds.
    pub fn max_length(&self) -> f64 {
        self.max
    }

    /// Changes whenever the enabled rules do (for caches).
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// Every interatomic distance up to `cutoff` (Å) for each element pair,
/// sorted, counting each bond once (periodic images included). Lets the
/// rule editor show the shortest distance of a pair and how many bonds a
/// range gives without searching again. None for structures too large to
/// search this way.
pub fn pair_distances(
    structure: &crate::model::Structure,
    cutoff: f64,
) -> Option<HashMap<(String, String), Vec<f64>>> {
    const MAX_ATOMS: usize = 3000;
    let n = structure.atoms.len();
    if n > MAX_ATOMS {
        return None;
    }
    let lat = structure.lattice;
    let shifts: Vec<[f64; 3]> = if structure.is_periodic {
        let mut v = Vec::with_capacity(27);
        for i in -1..=1 {
            for j in -1..=1 {
                for k in -1..=1 {
                    v.push([i as f64, j as f64, k as f64]);
                }
            }
        }
        v
    } else {
        vec![[0.0; 3]]
    };
    let offsets: Vec<[f64; 3]> = shifts
        .iter()
        .map(|s| crate::utils::linalg::frac_to_cart(*s, lat))
        .collect();

    let mut out: HashMap<(String, String), Vec<f64>> = HashMap::new();
    let c2 = cutoff * cutoff;
    for i in 0..n {
        let pi = structure.atoms[i].position;
        for j in i..n {
            let pj = structure.atoms[j].position;
            for (s, o) in shifts.iter().zip(&offsets) {
                // i == j: count each image pair once (one of ±shift) and skip
                // the atom itself.
                if i == j && (*s <= [0.0; 3]) {
                    continue;
                }
                let d2 = (0..3).map(|k| (pj[k] + o[k] - pi[k]).powi(2)).sum::<f64>();
                if d2 <= c2 && d2 > 1e-12 {
                    out.entry(pair_key(&structure.atoms[i].element, &structure.atoms[j].element))
                        .or_default()
                        .push(d2.sqrt());
                }
            }
        }
    }
    for v in out.values_mut() {
        v.sort_by(f64::total_cmp);
    }
    Some(out)
}

/// How many of the sorted `distances` a rule (min, max) counts as bonds.
pub fn count_in_range(distances: &[f64], min: f64, max: f64) -> usize {
    let lo = min.max(MIN_SEPARATION);
    distances.iter().filter(|&&d| d > lo && d <= max && d >= MIN_SEPARATION).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn els(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn rule<'a>(rules: &'a [BondRule], a: &str, b: &str) -> &'a BondRule {
        let k = pair_key(a, b);
        rules.iter().find(|r| (r.a.clone(), r.b.clone()) == k).unwrap()
    }

    #[test]
    fn vesta_lookup_ignores_order_and_prefers_covalent_oh() {
        assert_eq!(vesta_max("Ti", "O"), vesta_max("O", "Ti"));
        assert!((vesta_max("O", "Si").unwrap() - 1.99002).abs() < 1e-9);
        assert_eq!(vesta_max("H", "O"), Some(1.2));
        assert_eq!(vesta_max("Fe", "Fe"), None);
    }

    #[test]
    fn oxide_bonds_cation_anion_only() {
        let r = default_rules(&els(&["Ba", "Ti", "O", "O", "O"]), 1.15);
        assert_eq!(r.len(), 6); // Ba-Ba Ba-O Ba-Ti O-O O-Ti Ti-Ti
        assert!(rule(&r, "Ti", "O").enabled);
        assert_eq!(rule(&r, "Ti", "O").source, RuleSource::Vesta);
        assert!(rule(&r, "Ba", "O").enabled);
        assert!(!rule(&r, "Ba", "Ti").enabled);
        assert!(!rule(&r, "Ti", "Ti").enabled);
        // O–O is listed (peroxide, 1.7 Å) but far longer in an oxide.
        assert!(rule(&r, "O", "O").enabled);
    }

    #[test]
    fn metals_without_listed_partners_use_covalent_radii() {
        let r = default_rules(&els(&["Mn", "Ga", "Ni"]), 1.15);
        assert!(r.iter().all(|x| x.enabled && x.source == RuleSource::Covalent));
        let ni = rule(&r, "Ni", "Ni");
        assert!((ni.max - covalent_max("Ni", "Ni", 1.15)).abs() < 1e-12);
    }

    #[test]
    fn table_lookup_and_limits() {
        let t = BondTable::from_rules(&default_rules(&els(&["Ti", "O"]), 1.15));
        assert!(t.bonded("Ti", "O", 1.95));
        assert!(t.bonded("O", "Ti", 1.95));
        assert!(!t.bonded("Ti", "O", 2.5));
        assert!(!t.bonded("Ti", "O", 0.2)); // overlapping, never a bond
        assert!(!t.bonded("Ti", "Ti", 2.9));
        assert!(!t.bonded("Ti", "Xx", 1.0));
        assert!(t.max_length() >= 2.35);
    }

    #[test]
    fn distances_count_each_bond_once() {
        // Simple-cubic Po, a = 3.35 Å: six neighbours, i.e. 3 bonds per atom.
        let s = crate::model::Structure {
            lattice: [[3.35, 0.0, 0.0], [0.0, 3.35, 0.0], [0.0, 0.0, 3.35]],
            atoms: vec![crate::model::Atom {
                element: "Po".into(),
                position: [0.0; 3],
                original_index: 0,
                oxidation: None,
                occupancy: 1.0,
            }],
            formula: "Po".into(),
            is_periodic: true,
        };
        let d = pair_distances(&s, 3.5).unwrap();
        let v = &d[&pair_key("Po", "Po")];
        assert_eq!(v.len(), 3);
        assert_eq!(count_in_range(v, 0.0, 3.4), 3);
        assert_eq!(count_in_range(v, 0.0, 3.3), 0);
    }
}
