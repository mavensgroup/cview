// src/io/vasprun.rs
//
// VASP `vasprun.xml`: every ionic step of a relaxation or MD run.
//
// The file is scanned line by line rather than parsed as a DOM: an MD
// vasprun runs to gigabytes, and the parts needed here have a fixed layout.
//
//   <atominfo> <array name="atoms"> <rc><c>Fe</c><c>1</c></rc> ...
//   <calculation>
//     <scstep> ... </scstep>              electronic steps, ignored
//     <structure> basis (Å, rows) + positions (fractional) </structure>
//     <varray name="forces"> eV/Å </varray>
//     <energy> <i name="e_fr_energy"> </energy>
//   </calculation>
//
// Only `e_fr_energy` is read from the step-level <energy> block: VASP writes
// `e_wo_entrp` and `e_0_energy` there swapped, a long-standing quirk.

use crate::model::trajectory::{Frame, Trajectory};
use crate::model::Structure;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// True if `path` is a VASP XML document: its root `<modeling>` element
/// appears in the first few hundred bytes. Content, not name, decides, so
/// `vasprun.xml~`, `vasprun.xml_1` and `run3.xml` are all recognised.
pub fn sniff(path: &str) -> bool {
    use std::io::Read;
    let Ok(f) = File::open(path) else { return false };
    let mut head = Vec::with_capacity(512);
    if f.take(512).read_to_end(&mut head).is_err() {
        return false;
    }
    String::from_utf8_lossy(&head).contains("<modeling>")
}

/// Text between `<tag ...>` and `</tag>` on one line.
fn inner<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    let open = line.find(&format!("<{tag}"))?;
    let start = open + line[open..].find('>')? + 1;
    let end = start + line[start..].find(&format!("</{tag}>"))?;
    Some(&line[start..end])
}

fn vec3(line: &str) -> Option<[f64; 3]> {
    let v: Vec<f64> = inner(line, "v")?.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    (v.len() >= 3).then(|| [v[0], v[1], v[2]])
}

/// Read `n` `<v>` rows following the current line.
fn read_rows(lines: &mut impl Iterator<Item = io::Result<String>>, n: usize) -> io::Result<Vec<[f64; 3]>> {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let l = lines.next().ok_or_else(|| bad("file ends inside a <varray>"))??;
        if l.contains("</varray>") {
            break;
        }
        if let Some(v) = vec3(&l) {
            out.push(v);
        }
    }
    Ok(out)
}

#[derive(Default)]
struct Geometry {
    basis: Option<[[f64; 3]; 3]>,
    frac: Vec<[f64; 3]>,
}

/// Lattice rows and Cartesian positions.
type CellAndPositions = ([[f64; 3]; 3], Vec<[f64; 3]>);

impl Geometry {
    fn cartesian(&self) -> Option<CellAndPositions> {
        let b = self.basis?;
        let cart = self
            .frac
            .iter()
            .map(|f| [0, 1, 2].map(|k| f[0] * b[0][k] + f[1] * b[1][k] + f[2] * b[2][k]))
            .collect();
        Some((b, cart))
    }
}

/// Read a `<structure>` element (the `<structure` line has been consumed).
fn read_structure(lines: &mut impl Iterator<Item = io::Result<String>>, n: usize) -> io::Result<Geometry> {
    let mut g = Geometry::default();
    while let Some(l) = lines.next() {
        let l = l?;
        if l.contains("</structure>") {
            return Ok(g);
        }
        if l.contains("name=\"basis\"") {
            let rows = read_rows(lines, 3)?;
            if rows.len() == 3 {
                g.basis = Some([rows[0], rows[1], rows[2]]);
            }
        } else if l.contains("name=\"positions\"") {
            g.frac = read_rows(lines, n)?;
        }
    }
    Ok(g) // truncated file: whatever was read
}

/// Every ionic step. Falls back to the initial/final structures when the run
/// has no completed steps.
pub fn parse_trajectory(path: &str) -> io::Result<Trajectory> {
    let mut lines = BufReader::with_capacity(1 << 20, File::open(path)?).lines();
    let mut species: Vec<String> = Vec::new();
    let mut traj = Trajectory {
        species: vec![],
        frames: vec![],
        is_periodic: true,
        format: "vasprun.xml",
    };
    let mut initial: Option<Geometry> = None;
    let mut last_named: Option<Geometry> = None;

    // State of the <calculation> being read.
    let mut in_calc = false;
    let mut in_scstep = false;
    let mut geom: Option<Geometry> = None;
    let mut energy: Option<f64> = None;
    let mut max_force: Option<f64> = None;

    let push = |traj: &mut Trajectory, geom: Option<Geometry>, energy, max_force, species: &[String]| -> bool {
        let Some((lattice, positions)) = geom.as_ref().and_then(Geometry::cartesian) else {
            return true;
        };
        if traj.species.is_empty() {
            traj.species = species.to_vec();
        }
        let step = Some(traj.frames.len() as i64 + 1);
        traj.push_checked(Frame { lattice, positions, energy, max_force, step })
    };

    while let Some(line) = lines.next() {
        let line = line?;
        let t = line.trim_start();

        if t.starts_with("<array name=\"atoms\"") && species.is_empty() {
            for l in lines.by_ref() {
                let l = l?;
                if l.contains("</array>") {
                    break;
                }
                if l.contains("<rc>") {
                    if let Some(el) = inner(&l, "c") {
                        species.push(el.trim().to_string());
                    }
                }
            }
        } else if t.starts_with("<calculation>") {
            in_calc = true;
            geom = None;
            energy = None;
            max_force = None;
        } else if t.starts_with("</calculation>") {
            in_calc = false;
            if !push(&mut traj, geom.take(), energy, max_force, &species) {
                crate::utils::console::log_warn("vasprun.xml: atom count changes between steps; reading stopped");
                break;
            }
        } else if t.starts_with("<scstep>") {
            in_scstep = true;
        } else if t.starts_with("</scstep>") {
            in_scstep = false;
        } else if t.starts_with("<structure") {
            let g = read_structure(&mut lines, species.len())?;
            if in_calc {
                geom = Some(g);
            } else if t.contains("\"initialpos\"") {
                initial = Some(g);
            } else if t.contains("\"finalpos\"") {
                last_named = Some(g);
            }
        } else if in_calc && !in_scstep && t.starts_with("<varray name=\"forces\"") {
            let f = read_rows(&mut lines, species.len())?;
            max_force = f.iter().map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()).reduce(f64::max);
        } else if in_calc && !in_scstep && t.contains("name=\"e_fr_energy\"") {
            energy = inner(t, "i").and_then(|v| v.trim().parse().ok());
        }
    }

    // A run killed mid-step still has the geometry it was working on.
    if in_calc && geom.is_some() {
        push(&mut traj, geom.take(), energy, max_force, &species);
    }
    if traj.frames.is_empty() {
        for g in [initial, last_named].into_iter().flatten() {
            push(&mut traj, Some(g), None, None, &species);
        }
    }
    if species.is_empty() {
        return Err(bad("vasprun.xml has no <atominfo> atom list"));
    }
    if traj.frames.is_empty() {
        return Err(bad("vasprun.xml has no structures"));
    }
    Ok(traj)
}

/// The last ionic step (the relaxed structure).
pub fn parse(path: &str) -> io::Result<Structure> {
    let t = parse_trajectory(path)?;
    Ok(t.structure_at(t.len() - 1).expect("non-empty"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const XML: &str = r#"<?xml version="1.0" encoding="ISO-8859-1"?>
<modeling>
 <atominfo>
  <atoms>       2 </atoms>
  <array name="atoms" >
   <set>
    <rc><c>Fe</c><c>   1</c></rc>
    <rc><c>O </c><c>   2</c></rc>
   </set>
  </array>
 </atominfo>
 <structure name="initialpos" >
  <crystal>
   <varray name="basis" >
    <v>       4.0 0.0 0.0 </v>
    <v>       0.0 4.0 0.0 </v>
    <v>       0.0 0.0 4.0 </v>
   </varray>
  </crystal>
  <varray name="positions" >
   <v> 0.0 0.0 0.0 </v>
   <v> 0.5 0.5 0.5 </v>
  </varray>
 </structure>
 <calculation>
  <scstep>
   <energy>
    <i name="e_fr_energy">    532.0 </i>
   </energy>
  </scstep>
  <structure>
   <crystal>
    <varray name="basis" >
     <v> 4.0 0.0 0.0 </v>
     <v> 0.0 4.0 0.0 </v>
     <v> 0.0 0.0 4.0 </v>
    </varray>
   </crystal>
   <varray name="positions" >
    <v> 0.0 0.0 0.0 </v>
    <v> 0.5 0.5 0.5 </v>
   </varray>
  </structure>
  <varray name="forces" >
   <v> 0.3 0.4 0.0 </v>
   <v> 0.0 0.0 -0.1 </v>
  </varray>
  <energy>
   <i name="e_fr_energy">    -10.5 </i>
   <i name="e_wo_entrp">    -10.4 </i>
  </energy>
 </calculation>
 <calculation>
  <structure>
   <crystal>
    <varray name="basis" >
     <v> 4.2 0.0 0.0 </v>
     <v> 0.0 4.2 0.0 </v>
     <v> 0.0 0.0 4.2 </v>
    </varray>
   </crystal>
   <varray name="positions" >
    <v> 0.0 0.0 0.0 </v>
    <v> 0.5 0.5 0.4 </v>
   </varray>
  </structure>
"#;

    fn tmp(name: &str, body: &str) -> String {
        // One directory per file: tests run in parallel and clean up after themselves.
        let dir = std::env::temp_dir().join(format!("cview_vasprun_{}_{}", std::process::id(), name.replace(['.', '~'], "_")));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::File::create(&p).unwrap().write_all(body.as_bytes()).unwrap();
        p.to_string_lossy().to_string()
    }

    #[test]
    fn recognised_by_content_whatever_the_name() {
        for name in ["vasprun.xml~", "vasprun.xml_1", "run3.xml", "noext"] {
            assert!(sniff(&tmp(name, XML)), "{name}");
        }
        assert!(!sniff(&tmp("other.xml", "<?xml version=\"1.0\"?>\n<svg>\n")));
        for name in ["vasprun.xml~", "vasprun.xml_1", "run3.xml", "noext", "other.xml"] {
            let _ = std::fs::remove_dir_all(std::path::Path::new(&tmp(name, "")).parent().unwrap());
        }
    }

    #[test]
    fn steps_energies_forces_and_a_truncated_tail() {
        let p = tmp("vasprun.xml", XML);
        assert!(sniff(&p));
        let t = parse_trajectory(&p).unwrap();
        assert_eq!(t.species, vec!["Fe", "O"]);
        assert_eq!(t.len(), 2, "complete step + the one cut off at EOF");
        assert_eq!(t.frames[0].energy, Some(-10.5), "step energy, not the scstep one");
        assert!((t.frames[0].max_force.unwrap() - 0.5).abs() < 1e-12);
        let p1 = t.frames[1].positions[1];
        assert!((p1[2] - 0.4 * 4.2).abs() < 1e-12);
        assert_eq!(t.frames[1].lattice[0][0], 4.2);
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }
}
