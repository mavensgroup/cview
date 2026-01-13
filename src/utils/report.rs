// src/utils/report.rs

use crate::model::structure::Structure;
use crate::utils::geometry;
use std::collections::{HashMap, HashSet};

/// Generates the text for the "Interactions" tab when a file is loaded
pub fn structure_summary(structure: &Structure, filename: &str) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for atom in &structure.atoms {
        *counts.entry(atom.element.clone()).or_insert(0) += 1;
    }

    let mut parts: Vec<_> = counts.into_iter().collect();
    parts.sort_by(|a, b| a.0.cmp(&b.0));

    let formula_str: String = parts
        .iter()
        .map(|(el, count)| format!("{}{}", el, count))
        .collect::<Vec<_>>()
        .join(" ");

    let mut out = String::new();
    out.push_str(&format!("File: {}\n", filename));
    out.push_str(&format!("Formula: {}\n", formula_str));
    out.push_str("--------------------------------------------------\n");
    out.push_str(&format!(
        "{:<8} {:<8} {:<10} {:<10} {:<10}\n",
        "Index", "Element", "X", "Y", "Z"
    ));
    out.push_str("--------------------------------------------------\n");

    for (i, atom) in structure.atoms.iter().take(20).enumerate() {
        out.push_str(&format!(
            "{:<8} {:<8} {:<10.4} {:<10.4} {:<10.4}\n",
            i, atom.element, atom.position[0], atom.position[1], atom.position[2]
        ));
    }

    if structure.atoms.len() > 20 {
        out.push_str(&format!("... and {} more atoms.\n", structure.atoms.len() - 20));
    }

    out
}

/// Generates the text for the "Interactions" tab when atoms are selected
pub fn geometry_analysis(structure: &Structure, selected_indices: &HashSet<usize>) -> String {
    let mut sel: Vec<usize> = selected_indices.iter().cloned().collect();
    sel.sort();

    if sel.is_empty() {
        return "Select atoms to measure.".to_string();
    }

    let mut out = String::new();
    out.push_str("Selection:\n");

    for (i, &idx) in sel.iter().enumerate() {
        if let Some(atom) = structure.atoms.get(idx) {
            if i > 0 {
                out.push_str(" - ");
            }
            out.push_str(&format!("Atom (#{}, {})", idx, atom.element));
        }
    }
    out.push_str("\n\n");

    match sel.len() {
        2 => {
            let p1 = structure.atoms[sel[0]].position;
            let p2 = structure.atoms[sel[1]].position;
            let d = geometry::calculate_distance(p1, p2);
            out.push_str(&format!("Distance: {:.5} Å", d));
        }
        3 => {
            let p1 = structure.atoms[sel[0]].position;
            let p2 = structure.atoms[sel[1]].position;
            let p3 = structure.atoms[sel[2]].position;

            let angle = geometry::calculate_angle(p1, p2, p3);
            let d1 = geometry::calculate_distance(p1, p2);
            let d2 = geometry::calculate_distance(p2, p3);

            out.push_str(&format!("Angle (A-B-C): {:.2}°\n", angle));
            out.push_str(&format!("Dist (A-B):    {:.4} Å\n", d1));
            out.push_str(&format!("Dist (B-C):    {:.4} Å", d2));
        }
        4 => {
            let p1 = structure.atoms[sel[0]].position;
            let p2 = structure.atoms[sel[1]].position;
            let p3 = structure.atoms[sel[2]].position;
            let p4 = structure.atoms[sel[3]].position;

            let dihedral = geometry::calculate_dihedral(p1, p2, p3, p4);
            let angle = geometry::calculate_angle(p1, p2, p3);

            out.push_str(&format!("Dihedral:      {:.2}°\n", dihedral));
            out.push_str(&format!("Angle (A-B-C): {:.2}°", angle));
        }
        _ => {
            out.push_str("Select 2-4 atoms for geometric calculations.");
        }
    }
    out
}
