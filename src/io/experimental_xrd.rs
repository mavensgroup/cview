use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct ExperimentalData {
    pub name: String,
    pub points: Vec<(f64, f64)>, // (2Theta, Intensity)
}

pub fn parse(path: &str) -> io::Result<ExperimentalData> {
    let path_obj = Path::new(path);
    let name = path_obj.file_stem().unwrap_or_default().to_string_lossy().to_string();

    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut points = Vec::new();

    // Heuristic: Check file extension to decide how to parse
    // For .ASC (XY format), it's usually just "Angle  Intensity"

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        // Skip comments or empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('*') {
            continue;
        }

        // Try to parse two numbers
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 {
            if let (Ok(x), Ok(y)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                points.push((x, y));
            }
        }
    }

    if points.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "No valid data points found"));
    }

    // Normalize Intensity (0-100) for easier comparison
    let max_y = points.iter().map(|p| p.1).fold(0.0, f64::max);
    if max_y > 0.0 {
        for p in &mut points {
            p.1 = (p.1 / max_y) * 100.0;
        }
    }

    Ok(ExperimentalData { name, points })
}
