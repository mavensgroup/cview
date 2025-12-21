use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use crate::state::{AppState, RotationCenter, ExportFormat, RenderStyle};

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub rotation_mode: RotationCenter,
    pub load_conventional: bool,
    pub default_export_format: ExportFormat,
    // We can choose to save style or not.
    // Since you asked for it to be temporary, we won't strictly enforce loading it,
    // but including it here allows future persistence if you change your mind.
    #[serde(default)]
    pub style: RenderStyle,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rotation_mode: RotationCenter::Centroid,
            load_conventional: false,
            default_export_format: ExportFormat::Png, // Fixed: Png instead of PNG
            style: RenderStyle::default(),
        }
    }
}

pub fn load_config() -> Config {
    let config_path = "cview_config.json";
    if Path::new(config_path).exists() {
        if let Ok(content) = fs::read_to_string(config_path) {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
    }
    Config::default()
}

pub fn save_config(state: &AppState) {
    let config = Config {
        rotation_mode: state.rotation_mode,
        load_conventional: state.load_conventional,
        default_export_format: state.default_export_format,
        style: state.style,
    };

    if let Ok(json) = serde_json::to_string_pretty(&config) {
        let _ = fs::write("cview_config.json", json);
    }
}
