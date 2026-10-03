// src/io/lammps_dump.rs
//
// LAMMPS text dump (`dump ... atom/custom`, `.lammpstrj`, `.dump`): the first
// frame only. Playback of the remaining frames is a separate piece of work.
//
//   ITEM: TIMESTEP / NUMBER OF ATOMS / BOX BOUNDS [xy xz yz | abc origin] pp pp pp
//   ITEM: ATOMS id type x y z ...
//
// Positions may be wrapped (`x`), scaled (`xs`), unwrapped (`xu`) or scaled
// unwrapped (`xsu`); boxes may be orthogonal, restricted triclinic or the
// general `abc origin` form. Species come from an `element` column, else from
// a `mass` column, else are named `Type n`.

use crate::model::{Atom, Structure};
use nalgebra::{Matrix3, Vector3};
use std::fs::File;
use std::io::{self, BufRead, BufReader};

/// True if `path` begins like a LAMMPS dump, whatever its extension
/// (`.dat`, `.dump`, `.lammpstrj`, none).
pub fn sniff(path: &str) -> bool {
    let Ok(f) = File::open(path) else {
        return false;
    };
    let mut line = String::new();
    BufReader::new(f).read_line(&mut line).is_ok() && line.trim_start().starts_with("ITEM: TIMESTEP")
}

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn next_line(lines: &mut impl Iterator<Item = io::Result<String>>, what: &str) -> io::Result<String> {
    lines.next().ok_or_else(|| bad(format!("file ends inside {what}")))?
}

struct Boxed {
    cell: [[f64; 3]; 3],
    origin: [f64; 3],
    periodic: bool,
}

fn parse_box(item: &str, lines: &mut impl Iterator<Item = io::Result<String>>) -> io::Result<Boxed> {
    let flags: Vec<&str> = item.split_whitespace().skip(2).collect();
    let periodic = flags
        .iter()
        .filter(|t| t.len() == 2 && t.chars().all(|c| "pfsm".contains(c)))
        .any(|t| t.starts_with('p'));
    let mut rows = [[0.0f64; 6]; 3];
    for row in rows.iter_mut() {
        let l = next_line(lines, "BOX BOUNDS")?;
        let v: Vec<f64> = l.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        if v.len() < 2 {
            return Err(bad(format!("bad BOX BOUNDS line: '{}'", l.trim())));
        }
        row[..v.len().min(6)].copy_from_slice(&v[..v.len().min(6)]);
    }
    if flags.contains(&"abc") {
        // ax ay az ox oy oz, one line per lattice vector
        return Ok(Boxed {
            cell: [
                [rows[0][0], rows[0][1], rows[0][2]],
                [rows[1][0], rows[1][1], rows[1][2]],
                [rows[2][0], rows[2][1], rows[2][2]],
            ],
            origin: [rows[0][3], rows[0][4], rows[0][5]],
            periodic,
        });
    }
    if flags.contains(&"xy") {
        // Bounding box plus tilt factors: recover the true box edges.
        let (xy, xz, yz) = (rows[0][2], rows[1][2], rows[2][2]);
        let xlo = rows[0][0] - 0f64.min(xy).min(xz).min(xy + xz);
        let xhi = rows[0][1] - 0f64.max(xy).max(xz).max(xy + xz);
        let ylo = rows[1][0] - 0f64.min(yz);
        let yhi = rows[1][1] - 0f64.max(yz);
        let (zlo, zhi) = (rows[2][0], rows[2][1]);
        return Ok(Boxed {
            cell: [[xhi - xlo, 0.0, 0.0], [xy, yhi - ylo, 0.0], [xz, yz, zhi - zlo]],
            origin: [xlo, ylo, zlo],
            periodic,
        });
    }
    Ok(Boxed {
        cell: [
            [rows[0][1] - rows[0][0], 0.0, 0.0],
            [0.0, rows[1][1] - rows[1][0], 0.0],
            [0.0, 0.0, rows[2][1] - rows[2][0]],
        ],
        origin: [rows[0][0], rows[1][0], rows[2][0]],
        periodic,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Col {
    Id,
    Type,
    Element,
    Mass,
    Wrapped(usize),
    Scaled(usize),
    Unwrapped(usize),
    ScaledUnwrapped(usize),
    Other,
}

fn classify(name: &str) -> Col {
    let axis = |c: char| match c {
        'x' => Some(0),
        'y' => Some(1),
        'z' => Some(2),
        _ => None,
    };
    match name {
        "id" => return Col::Id,
        "type" => return Col::Type,
        "element" => return Col::Element,
        "mass" => return Col::Mass,
        _ => {}
    }
    let c: Vec<char> = name.chars().collect();
    let found = match c.as_slice() {
        [a] => axis(*a).map(Col::Wrapped),
        [a, 's'] => axis(*a).map(Col::Scaled),
        [a, 'u'] => axis(*a).map(Col::Unwrapped),
        [a, 's', 'u'] => axis(*a).map(Col::ScaledUnwrapped),
        _ => None,
    };
    found.unwrap_or(Col::Other)
}

/// Read the first frame of a dump as a `Structure`.
pub fn parse(path: &str) -> io::Result<Structure> {
    let mut lines = BufReader::with_capacity(1 << 16, File::open(path)?).lines();
    let mut n_atoms = None;
    let mut bx = None;
    let mut step = 0i64;

    while let Some(line) = lines.next() {
        let line = line?;
        let Some(item) = line.trim().strip_prefix("ITEM:").map(str::trim) else {
            if line.trim().is_empty() {
                continue;
            }
            return Err(bad(format!("expected an ITEM: line, found '{}'", line.trim())));
        };
        if item.starts_with("TIMESTEP") {
            step = next_line(&mut lines, "TIMESTEP")?.trim().parse().unwrap_or(0);
        } else if item.starts_with("NUMBER OF ATOMS") {
            n_atoms = Some(
                next_line(&mut lines, "NUMBER OF ATOMS")?
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| bad("bad NUMBER OF ATOMS"))?,
            );
        } else if item.starts_with("BOX BOUNDS") {
            bx = Some(parse_box(item, &mut lines)?);
        } else if let Some(cols) = item.strip_prefix("ATOMS") {
            let n = n_atoms.ok_or_else(|| bad("ATOMS before NUMBER OF ATOMS"))?;
            let bx = bx.ok_or_else(|| bad("ATOMS before BOX BOUNDS"))?;
            let roles: Vec<Col> = cols.split_whitespace().map(classify).collect();
            return read_atoms(&mut lines, &roles, n, &bx, step);
        } else if !(item.starts_with("UNITS") || item == "TIME") {
            return Err(bad(format!("unsupported dump section 'ITEM: {item}'")));
        } else {
            next_line(&mut lines, item)?; // UNITS / TIME carry one value line
        }
    }
    Err(bad("no ITEM: ATOMS section in LAMMPS dump"))
}

fn read_atoms(
    lines: &mut impl Iterator<Item = io::Result<String>>,
    roles: &[Col],
    n: usize,
    bx: &Boxed,
    step: i64,
) -> io::Result<Structure> {
    let has = |f: fn(&Col) -> bool| roles.iter().any(f);
    if !has(|c| matches!(c, Col::Wrapped(_) | Col::Scaled(_) | Col::Unwrapped(_) | Col::ScaledUnwrapped(_))) {
        return Err(bad("dump has no position columns (x y z / xs ys zs / xu yu zu)"));
    }
    let only_unwrapped = !has(|c| matches!(c, Col::Wrapped(_) | Col::Scaled(_)));

    let mut ids = Vec::with_capacity(n);
    let mut types = Vec::with_capacity(n);
    let mut elements = Vec::new();
    let mut masses = Vec::new();
    let nan = [f64::NAN; 3];
    let (mut wrapped, mut scaled, mut unwrapped, mut scaled_unw) =
        (vec![nan; n], vec![nan; n], vec![nan; n], vec![nan; n]);

    for k in 0..n {
        let l = next_line(lines, "ATOMS")?;
        let mut it = l.split_ascii_whitespace();
        for role in roles {
            let tok = it.next().ok_or_else(|| bad(format!("short atom line: '{}'", l.trim())))?;
            let num = || tok.parse::<f64>().map_err(|_| bad(format!("bad number '{tok}'")));
            match *role {
                Col::Id => ids.push(tok.parse::<i64>().map_err(|_| bad("bad atom id"))?),
                Col::Type => types.push(tok.to_string()),
                Col::Element => elements.push(tok.to_string()),
                Col::Mass => masses.push(num()?),
                Col::Wrapped(a) => wrapped[k][a] = num()?,
                Col::Scaled(a) => scaled[k][a] = num()?,
                Col::Unwrapped(a) => unwrapped[k][a] = num()?,
                Col::ScaledUnwrapped(a) => scaled_unw[k][a] = num()?,
                Col::Other => {}
            }
        }
    }

    // Lattice vectors as columns, so `m * frac` is Cartesian.
    let c = bx.cell;
    let m = Matrix3::new(c[0][0], c[1][0], c[2][0], c[0][1], c[1][1], c[2][1], c[0][2], c[1][2], c[2][2]);
    let origin = Vector3::new(bx.origin[0], bx.origin[1], bx.origin[2]);
    let finite = |p: &[f64; 3]| p.iter().all(|v| v.is_finite());
    let from_scaled = |s: &[f64; 3]| {
        let r = m * Vector3::new(s[0], s[1], s[2]);
        [r.x, r.y, r.z]
    };
    let shifted = |p: &[f64; 3]| [p[0] - origin.x, p[1] - origin.y, p[2] - origin.z];

    let mut pos: Vec<[f64; 3]> = (0..n)
        .map(|k| {
            if finite(&wrapped[k]) {
                shifted(&wrapped[k])
            } else if finite(&scaled[k]) {
                from_scaled(&scaled[k])
            } else if finite(&unwrapped[k]) {
                shifted(&unwrapped[k])
            } else {
                from_scaled(&scaled_unw[k])
            }
        })
        .collect();
    // Unwrapped atoms can lie far outside the box: fold them back in for display.
    if only_unwrapped && bx.periodic {
        if let Some(inv) = m.try_inverse() {
            for p in pos.iter_mut() {
                let f = inv * Vector3::new(p[0], p[1], p[2]);
                let r = m * f.map(|x| x - x.floor());
                *p = [r.x, r.y, r.z];
            }
        }
    }

    let species: Vec<String> = if elements.len() == n {
        elements
    } else if masses.len() == n && masses.iter().all(|&w| element_from_mass(w).is_some()) {
        masses.iter().map(|&w| element_from_mass(w).unwrap().to_string()).collect()
    } else if types.len() == n {
        types.iter().map(|t| format!("Type {t}")).collect()
    } else {
        vec!["Type 1".to_string(); n]
    };

    let mut order: Vec<usize> = (0..n).collect();
    if ids.len() == n {
        order.sort_by_key(|&k| ids[k]);
    }
    let atoms = order
        .iter()
        .enumerate()
        .map(|(i, &k)| Atom {
            element: species[k].clone(),
            position: pos[k],
            original_index: i,
            oxidation: None,
            occupancy: 1.0,
        })
        .collect();

    Ok(Structure {
        lattice: bx.cell,
        atoms,
        formula: format!("LAMMPS step {step}"),
        is_periodic: bx.periodic,
    })
}

/// Element whose standard atomic weight is within 3% of `mass`.
fn element_from_mass(mass: f64) -> Option<&'static str> {
    const W: [(&str, f64); 54] = [
        ("H", 1.008), ("He", 4.0026), ("Li", 6.94), ("Be", 9.0122), ("B", 10.81), ("C", 12.011),
        ("N", 14.007), ("O", 15.999), ("F", 18.998), ("Ne", 20.18), ("Na", 22.99), ("Mg", 24.305),
        ("Al", 26.982), ("Si", 28.085), ("P", 30.974), ("S", 32.06), ("Cl", 35.45), ("Ar", 39.948),
        ("K", 39.098), ("Ca", 40.078), ("Sc", 44.956), ("Ti", 47.867), ("V", 50.942), ("Cr", 51.996),
        ("Mn", 54.938), ("Fe", 55.845), ("Co", 58.933), ("Ni", 58.693), ("Cu", 63.546), ("Zn", 65.38),
        ("Ga", 69.723), ("Ge", 72.63), ("As", 74.922), ("Se", 78.971), ("Br", 79.904), ("Kr", 83.798),
        ("Rb", 85.468), ("Sr", 87.62), ("Y", 88.906), ("Zr", 91.224), ("Nb", 92.906), ("Mo", 95.95),
        ("Ru", 101.07), ("Rh", 102.91), ("Pd", 106.42), ("Ag", 107.87), ("Cd", 112.41), ("In", 114.82),
        ("Sn", 118.71), ("Sb", 121.76), ("Te", 127.6), ("I", 126.9), ("Xe", 131.29), ("Cs", 132.91),
    ];
    if !(mass > 0.5) {
        return None;
    }
    W.iter()
        .filter(|(_, w)| ((mass - w) / w).abs() < 0.03)
        .min_by(|a, b| (mass - a.1).abs().total_cmp(&(mass - b.1).abs()))
        .map(|(e, _)| *e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(name: &str, body: &str) -> String {
        let dir = std::env::temp_dir().join(format!("cview_lmp_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::File::create(&p).unwrap().write_all(body.as_bytes()).unwrap();
        p.to_string_lossy().to_string()
    }

    const HEAD: &str = "ITEM: TIMESTEP\n7\nITEM: NUMBER OF ATOMS\n2\nITEM: BOX BOUNDS pp pp pp\n-5 5\n-5 5\n-5 5\n";

    fn near(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn first_frame_sorted_by_id_with_origin_removed() {
        // A second frame must be ignored.
        let body = format!("{HEAD}ITEM: ATOMS id type x y z\n2 1 -4 0 0\n1 2 1 2 3\n{HEAD}ITEM: ATOMS id type x y z\n1 1 0 0 0\n2 1 0 0 0\n");
        let p = write_tmp("a.dat", &body);
        assert!(sniff(&p));
        let s = parse(&p).unwrap();
        assert_eq!(s.atoms.len(), 2);
        assert!(near(s.atoms[0].position, [6.0, 7.0, 8.0]));
        assert!(near(s.atoms[1].position, [1.0, 5.0, 5.0]));
        assert_eq!(s.atoms[0].element, "Type 2");
        assert_eq!(s.lattice[0], [10.0, 0.0, 0.0]);
        assert!(s.is_periodic);
    }

    #[test]
    fn scaled_and_unwrapped_columns() {
        let xs = write_tmp("xs.dat", &format!("{HEAD}ITEM: ATOMS id type xs ys zs\n1 1 0.5 0.5 0.5\n2 1 0 0 0\n"));
        assert!(near(parse(&xs).unwrap().atoms[0].position, [5.0, 5.0, 5.0]));
        let xu = write_tmp("xu.dat", &format!("{HEAD}ITEM: ATOMS id type xu yu zu\n1 1 -12 0 0\n2 1 0 0 0\n"));
        assert!(near(parse(&xu).unwrap().atoms[0].position, [3.0, 5.0, 5.0]));
    }

    #[test]
    fn triclinic_box_and_element_column() {
        let body = "ITEM: TIMESTEP\n0\nITEM: NUMBER OF ATOMS\n1\nITEM: BOX BOUNDS xy xz yz pp pp pp\n0 12 2\n0 10 0\n0 10 0\nITEM: ATOMS id type element x y z\n1 1 Fe 1 1 1\n";
        let s = parse(&write_tmp("tri.dat", body)).unwrap();
        assert_eq!(s.lattice, [[10.0, 0.0, 0.0], [2.0, 10.0, 0.0], [0.0, 0.0, 10.0]]);
        assert_eq!(s.atoms[0].element, "Fe");
    }

    #[test]
    fn mass_identifies_elements() {
        assert_eq!(element_from_mass(55.85), Some("Fe"));
        assert_eq!(element_from_mass(0.0), None);
    }
}
