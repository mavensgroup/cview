// src/physics/bond_valence/database.rs

use std::collections::HashMap;
use std::sync::OnceLock;

/// Bond Valence Parameter
#[derive(Debug, Clone, Copy)]
pub struct BVParam {
    pub r0: f64,      // Reference bond length (Å)
    pub b: f64,       // Softness parameter
    pub source: ParamSource,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamSource {
    Experimental,  // From Brown & Altermatt (1985) / IUCr database
    Predicted,     // ML-generated for unknown pairs
}

// Global database (initialized once at startup)
static BV_DATABASE: OnceLock<HashMap<(String, String), BVParam>> = OnceLock::new();

/// Initialize the bond valence parameter database
pub fn init_database() -> &'static HashMap<(String, String), BVParam> {
    BV_DATABASE.get_or_init(|| {
        let mut db = HashMap::new();
        load_embedded_params(&mut db);
        db
    })
}

/// Load embedded parameters (top 200+ most common pairs)
/// Source: Brown & Altermatt (1985), Brese & O'Keeffe (1991)
/// IUCr database: https://www.iucr.org/resources/data/datasets/bond-valence-parameters
fn load_embedded_params(db: &mut HashMap<(String, String), BVParam>) {
    // Standard B parameter for most bonds
    let std_b = 0.37;
    
    // Macro to add symmetric pairs
    macro_rules! add_param {
        ($cat:expr, $an:expr, $r0:expr) => {
            let param = BVParam { r0: $r0, b: std_b, source: ParamSource::Experimental };
            db.insert(($cat.into(), $an.into()), param);
            db.insert(($an.into(), $cat.into()), param);
        };
        ($cat:expr, $an:expr, $r0:expr, $b:expr) => {
            let param = BVParam { r0: $r0, b: $b, source: ParamSource::Experimental };
            db.insert(($cat.into(), $an.into()), param);
            db.insert(($an.into(), $cat.into()), param);
        };
    }
    
    // === OXIDES (Most common in crystallography) ===
    
    // Group 1 (Alkali metals) with O
    add_param!("H", "O", 0.989);
    add_param!("Li", "O", 1.466);
    add_param!("Na", "O", 1.803);
    add_param!("K", "O", 2.132);
    add_param!("Rb", "O", 2.263);
    add_param!("Cs", "O", 2.417);
    
    // Group 2 (Alkaline earth) with O
    add_param!("Be", "O", 1.381);
    add_param!("Mg", "O", 1.693);
    add_param!("Ca", "O", 1.967);
    add_param!("Sr", "O", 2.118);
    add_param!("Ba", "O", 2.285);
    add_param!("Ra", "O", 2.420);
    
    // 3d Transition metals with O (critical for batteries, catalysts)
    add_param!("Sc", "O", 1.849);
    add_param!("Ti", "O", 1.815);
    add_param!("V", "O", 1.743);
    add_param!("Cr", "O", 1.724);
    add_param!("Mn", "O", 1.790);
    add_param!("Fe", "O", 1.759);
    add_param!("Co", "O", 1.692);
    add_param!("Ni", "O", 1.654);
    add_param!("Cu", "O", 1.679);
    add_param!("Zn", "O", 1.704);
    
    // 4d Transition metals with O
    add_param!("Y", "O", 2.019);
    add_param!("Zr", "O", 1.937);
    add_param!("Nb", "O", 1.911);
    add_param!("Mo", "O", 1.907);
    add_param!("Tc", "O", 1.859);
    add_param!("Ru", "O", 1.834);
    add_param!("Rh", "O", 1.812);
    add_param!("Pd", "O", 1.792);
    add_param!("Ag", "O", 1.842);
    add_param!("Cd", "O", 1.904);
    
    // 5d Transition metals with O
    add_param!("La", "O", 2.172);
    add_param!("Hf", "O", 1.923);
    add_param!("Ta", "O", 1.920);
    add_param!("W", "O", 1.921);
    add_param!("Re", "O", 1.891);
    add_param!("Os", "O", 1.856);
    add_param!("Ir", "O", 1.847);
    add_param!("Pt", "O", 1.837);
    add_param!("Au", "O", 1.833);
    add_param!("Hg", "O", 1.967);
    
    // p-block elements with O
    add_param!("B", "O", 1.371);
    add_param!("Al", "O", 1.651);
    add_param!("Ga", "O", 1.730);
    add_param!("In", "O", 1.902);
    add_param!("Tl", "O", 2.042);
    
    add_param!("C", "O", 1.394);
    add_param!("Si", "O", 1.624);
    add_param!("Ge", "O", 1.748);
    add_param!("Sn", "O", 1.905);
    add_param!("Pb", "O", 2.042);
    
    add_param!("N", "O", 1.432);
    add_param!("P", "O", 1.617);
    add_param!("As", "O", 1.767);
    add_param!("Sb", "O", 1.973);
    add_param!("Bi", "O", 2.094);
    
    add_param!("S", "O", 1.644);
    add_param!("Se", "O", 1.811);
    add_param!("Te", "O", 1.977);
    
    add_param!("Cl", "O", 1.674);
    add_param!("Br", "O", 1.849);
    add_param!("I", "O", 2.019);
    
    // === HALIDES (Important for ionic conductors) ===
    
    // With Fluorine
    add_param!("Li", "F", 1.360);
    add_param!("Na", "F", 1.677);
    add_param!("K", "F", 1.992);
    add_param!("Rb", "F", 2.150);
    add_param!("Cs", "F", 2.304);
    add_param!("Be", "F", 1.281);
    add_param!("Mg", "F", 1.578);
    add_param!("Ca", "F", 1.842);
    add_param!("Sr", "F", 1.993);
    add_param!("Ba", "F", 2.170);
    add_param!("Al", "F", 1.545);
    add_param!("Si", "F", 1.549);
    
    // With Chlorine
    add_param!("Li", "Cl", 1.949);
    add_param!("Na", "Cl", 2.237);
    add_param!("K", "Cl", 2.567);
    add_param!("Rb", "Cl", 2.715);
    add_param!("Cs", "Cl", 2.871);
    add_param!("Mg", "Cl", 2.107);
    add_param!("Ca", "Cl", 2.372);
    add_param!("Sr", "Cl", 2.527);
    add_param!("Ba", "Cl", 2.704);
    
    // With Bromine
    add_param!("Li", "Br", 2.117);
    add_param!("Na", "Br", 2.405);
    add_param!("K", "Br", 2.735);
    add_param!("Rb", "Br", 2.883);
    add_param!("Cs", "Br", 3.039);
    
    // With Iodine
    add_param!("Li", "I", 2.340);
    add_param!("Na", "I", 2.628);
    add_param!("K", "I", 2.958);
    add_param!("Rb", "I", 3.106);
    add_param!("Cs", "I", 3.262);
    
    // === RARE EARTH ELEMENTS (Phosphors, magnets) ===
    add_param!("La", "O", 2.172);
    add_param!("Ce", "O", 2.151);
    add_param!("Pr", "O", 2.134);
    add_param!("Nd", "O", 2.105);
    add_param!("Pm", "O", 2.086);
    add_param!("Sm", "O", 2.067);
    add_param!("Eu", "O", 2.074);
    add_param!("Gd", "O", 2.063);
    add_param!("Tb", "O", 2.038);
    add_param!("Dy", "O", 2.027);
    add_param!("Ho", "O", 2.010);
    add_param!("Er", "O", 1.997);
    add_param!("Tm", "O", 1.981);
    add_param!("Yb", "O", 1.985);
    add_param!("Lu", "O", 1.971);
    
    // === SULFIDES (Semiconductors, batteries) ===
    add_param!("Li", "S", 2.126);
    add_param!("Na", "S", 2.398);
    add_param!("K", "S", 2.778);
    add_param!("Mg", "S", 2.321);
    add_param!("Ca", "S", 2.597);
    add_param!("Fe", "S", 2.321);
    add_param!("Co", "S", 2.260);
    add_param!("Ni", "S", 2.222);
    add_param!("Cu", "S", 2.205);
    add_param!("Zn", "S", 2.272);
    
    // === NITRIDES (Hard materials, LEDs) ===
    add_param!("Li", "N", 1.756);
    add_param!("Mg", "N", 1.988);
    add_param!("Al", "N", 1.869);
    add_param!("Si", "N", 1.879);
    add_param!("Ti", "N", 2.041);
    add_param!("Ga", "N", 1.976);
    
    // === PHOSPHIDES (Semiconductors) ===
    add_param!("Li", "P", 2.362);
    add_param!("Na", "P", 2.649);
    add_param!("Ca", "P", 2.826);
    add_param!("Ga", "P", 2.265);
    add_param!("In", "P", 2.541);
    
    // === ACTINIDES (Nuclear materials) ===
    add_param!("Th", "O", 2.167);
    add_param!("U", "O", 2.051);
    add_param!("Np", "O", 2.035);
    add_param!("Pu", "O", 2.019);
}

/// Get bond valence parameter (with fallback to ML)
pub fn get_param(elem_a: &str, elem_b: &str) -> BVParam {
    let db = init_database();
    
    // Try direct lookup
    if let Some(&param) = db.get(&(elem_a.into(), elem_b.into())) {
        return param;
    }
    
    // Fallback: ML prediction
    super::ml_predictor::predict_param_ml(elem_a, elem_b)
}

/// Check if experimental parameters exist for this pair
pub fn has_experimental_param(elem_a: &str, elem_b: &str) -> bool {
    let db = init_database();
    db.contains_key(&(elem_a.into(), elem_b.into()))
}

/// Get all available cation-anion pairs in database
pub fn get_available_pairs() -> Vec<(String, String)> {
    let db = init_database();
    db.keys()
        .filter(|(a, b)| a < b) // Only return one direction to avoid duplicates
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_database_symmetry() {
        // Parameters should be symmetric
        let li_o = get_param("Li", "O");
        let o_li = get_param("O", "Li");
        
        assert!((li_o.r0 - o_li.r0).abs() < 1e-6);
        assert_eq!(li_o.source, ParamSource::Experimental);
    }
    
    #[test]
    fn test_common_oxides() {
        // Test critical battery materials
        let params = [
            ("Li", "O", 1.466),
            ("Fe", "O", 1.759),
            ("Co", "O", 1.692),
            ("Ni", "O", 1.654),
            ("Mn", "O", 1.790),
        ];
        
        for (cat, an, expected_r0) in params {
            let param = get_param(cat, an);
            assert!((param.r0 - expected_r0).abs() < 0.001,
                "{}-{}: expected {}, got {}", cat, an, expected_r0, param.r0);
        }
    }
    
    #[test]
    fn test_ml_fallback() {
        // Unknown pair should trigger ML prediction
        let param = get_param("Eu", "N");
        assert_eq!(param.source, ParamSource::Predicted);
        assert!(param.r0 > 0.5 && param.r0 < 3.5); // Reasonable range
    }
}
