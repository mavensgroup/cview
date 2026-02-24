// src/physics/bond_valence/mod.rs

pub mod calculator;
pub mod database;
pub mod ml_predictor;

// Re-export commonly used items
pub use calculator::{
    assess_structure_quality, calculate_bvs, calculate_bvs_all, calculate_bvs_all_pbc,
    calculate_bvs_deviation, calculate_bvs_pbc, calculate_structure_quality,
    get_ideal_oxidation_state, BVSQuality,
};
pub use database::{get_param, has_experimental_param, BVParam, ParamSource};
pub use ml_predictor::{explain_prediction, predict_param_ml, prediction_confidence};

#[cfg(feature = "parallel")]
pub use calculator::calculate_bvs_all_parallel;
