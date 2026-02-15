// src/physics/bond_valence/mod.rs

pub mod database;
pub mod ml_predictor;
pub mod calculator;

// Re-export commonly used items
pub use database::{BVParam, ParamSource, get_param, has_experimental_param};
pub use ml_predictor::{predict_param_ml, prediction_confidence, explain_prediction};
pub use calculator::{
    calculate_bvs,
    calculate_bvs_pbc,
    calculate_bvs_all,
    calculate_bvs_all_pbc,
    get_ideal_oxidation_state,
    calculate_bvs_deviation,
    calculate_structure_quality,
    assess_structure_quality,
    BVSQuality,
};

#[cfg(feature = "parallel")]
pub use calculator::calculate_bvs_all_parallel;
