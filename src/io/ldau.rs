// src/io/ldau.rs
//
// DFT+U on-site occupation matrices from a VASP OUTCAR (LDAUPRINT >= 1):
//
//   atom =  28  type =  1  l = 3
//
//    onsite density matrix
//
//   spin component  1
//
//     0.0353 -0.0004  0.0091 ...      (2l+1 rows of 2l+1 values)
//
//   spin component  2
//   ...
//
// VASP prints the blocks at every electronic step; the last block of each
// atom is the converged one. Rows and columns follow VASP's real spherical
// harmonics, m = -l..l (see `physics::analysis::orbitals`). Collinear runs
// only: a non-collinear OUTCAR (spin components 1..4, complex) is refused.

use nalgebra::DMatrix;

/// The converged on-site density matrix of one DFT+U atom.
#[derive(Clone, Debug, PartialEq)]
pub struct OnsiteMatrix {
    /// Atom index, from 0, in POSCAR order.
    pub atom: usize,
    /// Angular momentum of the +U shell.
    pub l: usize,
    /// One (2l+1)×(2l+1) matrix per spin: [up] or [up, down].
    pub spins: Vec<DMatrix<f64>>,
}

/// Parse every atom's last on-site density matrix, sorted by atom.
pub fn parse_onsite(text: &str) -> Result<Vec<OnsiteMatrix>, String> {
    let mut found: std::collections::BTreeMap<usize, OnsiteMatrix> = Default::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let Some((atom, l)) = block_header(lines[i]) else {
            i += 1;
            continue;
        };
        i += 1;
        let n = 2 * l + 1;
        let mut spins = Vec::new();
        // Up to the next header (or anything that is not part of the block).
        while i < lines.len() && block_header(lines[i]).is_none() {
            let t = lines[i].trim();
            if let Some(rest) = t.strip_prefix("spin component") {
                let label: Vec<&str> = rest.split_whitespace().collect();
                let k: usize = match label.as_slice() {
                    [k] => k.parse().map_err(|_| format!("OUTCAR: bad spin component '{t}'"))?,
                    _ => return Err("non-collinear occupation matrices are not supported".into()),
                };
                if k > 2 {
                    return Err("non-collinear occupation matrices are not supported".into());
                }
                let (m, next) = read_matrix(&lines, i + 1, n)
                    .ok_or_else(|| format!("OUTCAR: incomplete occupation matrix for atom {atom}"))?;
                spins.push(m);
                i = next;
                continue;
            }
            if t.starts_with("occupancies and eigenvectors") || t.starts_with("o =") {
                i += 1;
                continue;
            }
            if !t.is_empty() && !t.starts_with("onsite density matrix") {
                break;
            }
            i += 1;
        }
        if !spins.is_empty() {
            found.insert(atom - 1, OnsiteMatrix { atom: atom - 1, l, spins });
        }
    }
    Ok(found.into_values().collect())
}

/// `atom =  28  type =  1  l = 3` → (28, 3).
fn block_header(line: &str) -> Option<(usize, usize)> {
    let t = line.trim_start();
    let rest = t.strip_prefix("atom =")?;
    let (atom, rest) = rest.split_once("type =")?;
    let (_, l) = rest.split_once("l =")?;
    let atom: usize = atom.trim().parse().ok()?;
    let l: usize = l.trim().parse().ok()?;
    (atom >= 1 && l <= 3).then_some((atom, l))
}

/// `n` rows of `n` numbers starting at or after `start` (blank lines skipped).
fn read_matrix(lines: &[&str], start: usize, n: usize) -> Option<(DMatrix<f64>, usize)> {
    let mut m = DMatrix::zeros(n, n);
    let mut row = 0;
    let mut i = start;
    while row < n {
        let t = lines.get(i)?.trim();
        i += 1;
        if t.is_empty() {
            continue;
        }
        let vals = numbers(t);
        if vals.len() != n {
            return None;
        }
        for (c, v) in vals.into_iter().enumerate() {
            m[(row, c)] = v;
        }
        row += 1;
    }
    Some((m, i))
}

/// Whitespace-separated numbers, also splitting fixed-width fields that run
/// together ("-0.1234-0.0567").
fn numbers(t: &str) -> Vec<f64> {
    let mut out = Vec::new();
    for tok in t.split_whitespace() {
        if let Ok(v) = tok.parse() {
            out.push(v);
            continue;
        }
        let mut start = 0;
        for (k, c) in tok.char_indices().skip(1) {
            if c == '-' && !tok[..k].ends_with(['e', 'E']) {
                if let Ok(v) = tok[start..k].parse() {
                    out.push(v);
                }
                start = k;
            }
        }
        if let Ok(v) = tok[start..].parse() {
            out.push(v);
        }
    }
    out
}

/// The OUTCAR next to `path` (a CHGCAR, say), if there is one.
pub fn sibling_outcar(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let p = path.parent()?.join("OUTCAR");
    p.is_file().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "\
atom =   2  type =  1  l = 1

 onsite density matrix

spin component  1

  0.9000  0.0000 -0.1000
  0.0000  0.2000  0.0000
 -0.1000  0.0000  0.3000

spin component  2

  0.1000  0.0000  0.0000
  0.0000  0.1000  0.0000
  0.0000  0.0000  0.1000

 occupancies and eigenvectors

  o =  0.9142  v = -0.9898  0.0000  0.1424  0.0000  0.0000  0.0000
     EDDAV:  cpu time  210.9436: real time  211.7723
";

    #[test]
    fn reads_the_last_block_of_each_atom() {
        let early = BLOCK.replace("0.9000", "0.5000");
        let text = format!("{early}\nsomething else\n{BLOCK}");
        let m = parse_onsite(&text).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].atom, m[0].l, m[0].spins.len()), (1, 1, 2));
        assert_eq!(m[0].spins[0][(0, 0)], 0.9);
        assert_eq!(m[0].spins[0][(2, 0)], -0.1);
        assert_eq!(m[0].spins[1][(1, 1)], 0.1);
    }

    #[test]
    fn refuses_non_collinear() {
        let t = BLOCK.replace("spin component  1", "spin component  1  1");
        assert!(parse_onsite(&t).is_err());
    }

    #[test]
    fn splits_glued_fields() {
        assert_eq!(numbers("  0.1234-0.0567 -1.0e-02-3"), vec![0.1234, -0.0567, -0.01, -3.0]);
    }
}
