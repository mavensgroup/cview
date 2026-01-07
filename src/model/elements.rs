/// Returns (radius_in_angstroms, (r, g, b))
/// Radii are based on covalent radii. Colors are standard CPK.
/// Returns the Atomic Number (Z) for a given element symbol

pub fn get_atomic_number(element: &str) -> i32 {
    match element {
        // --- Period 1 ---
        "H"  => 1,
        "He" => 2,
        // --- Period 2 ---
        "Li" => 3, "Be" => 4, "B" => 5, "C" => 6, "N" => 7, "O" => 8, "F" => 9, "Ne" => 10,
        // --- Period 3 ---
        "Na" => 11, "Mg" => 12, "Al" => 13, "Si" => 14, "P" => 15, "S" => 16, "Cl" => 17, "Ar" => 18,
        // --- Period 4 ---
        "K" => 19, "Ca" => 20, "Sc" => 21, "Ti" => 22, "V" => 23, "Cr" => 24, "Mn" => 25,
        "Fe" => 26, "Co" => 27, "Ni" => 28, "Cu" => 29, "Zn" => 30, "Ga" => 31, "Ge" => 32,
        "As" => 33, "Se" => 34, "Br" => 35, "Kr" => 36,
        // ... add others as needed
        _ => 0, // Unknown/Dummy
    }
}
pub fn get_atom_properties(element: &str) -> (f64, (f64, f64, f64)) {
    match element {
        // --- Period 1 ---
        "H"  => (0.37, (1.00, 1.00, 1.00)), // White
        "He" => (0.32, (0.85, 1.00, 1.00)), // Cyan-White

        // --- Period 2 ---
        "Li" => (1.34, (0.80, 0.50, 1.00)), // Violet
        "Be" => (0.90, (0.76, 1.00, 0.00)), // Yellow-Green
        "B"  => (0.82, (1.00, 0.70, 0.70)), // Pink-Salmon
        "C"  => (0.77, (0.20, 0.20, 0.20)), // Dark Grey
        "N"  => (0.75, (0.19, 0.31, 0.97)), // Blue
        "O"  => (0.73, (1.00, 0.05, 0.05)), // Red
        "F"  => (0.71, (0.56, 0.88, 0.31)), // Green
        "Ne" => (0.69, (0.70, 0.89, 0.96)), // Light Cyan

        // --- Period 3 ---
        "Na" => (1.54, (0.67, 0.36, 0.95)), // Violet
        "Mg" => (1.30, (0.54, 1.00, 0.00)), // Forest Green
        "Al" => (1.18, (0.75, 0.65, 0.65)), // Silver-Grey
        "Si" => (1.11, (0.94, 0.78, 0.63)), // Tan
        "P"  => (1.06, (1.00, 0.50, 0.00)), // Orange
        "S"  => (1.02, (1.00, 1.00, 0.19)), // Yellow
        "Cl" => (0.99, (0.12, 0.94, 0.12)), // Bright Green
        "Ar" => (0.97, (0.50, 0.82, 0.89)), // Cyan

        // --- Period 4 (Selected Common Metals) ---
        "K"  => (1.96, (0.56, 0.25, 0.83)), // Purple
        "Ca" => (1.74, (0.24, 1.00, 0.00)), // Dark Green
        "Ti" => (1.36, (0.75, 0.76, 0.78)), // Silver
        "V"  => (1.25, (0.65, 0.65, 0.67)), // Grey
        "Cr" => (1.27, (0.54, 0.60, 0.78)), // Blue-Grey
        "Mn" => (1.39, (0.61, 0.48, 0.78)), // Purple-Grey
        "Fe" => (1.25, (0.88, 0.40, 0.20)), // Rust / Orange
        "Co" => (1.26, (0.94, 0.56, 0.63)), // Pink-ish
        "Ni" => (1.21, (0.31, 0.82, 0.31)), // Green
        "Cu" => (1.38, (0.78, 0.50, 0.20)), // Copper
        "Zn" => (1.31, (0.49, 0.50, 0.69)), // Slate
        "Ga" => (1.26, (0.76, 0.56, 0.56)), // Dark Pink
        "Ge" => (1.22, (0.40, 0.56, 0.56)), // Grey-Teal
        "As" => (1.19, (0.74, 0.50, 0.89)), // Violet
        "Se" => (1.16, (1.00, 0.63, 0.00)), // Orange
        "Br" => (1.14, (0.65, 0.16, 0.16)), // Brown
        "Kr" => (1.10, (0.36, 0.72, 0.82)), // Blue-Green

        // --- Period 5 (Selected) ---
        "Ag" => (1.53, (0.75, 0.75, 0.75)), // Silver
        "Au" => (1.44, (1.00, 0.82, 0.14)), // Gold

        // --- Catch-All (Unknown) ---
        _    => (1.00, (1.00, 0.08, 0.58)), // Hot Pink for errors
    }
}
