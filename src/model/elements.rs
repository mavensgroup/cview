// src/model/elements.rs

/// Holds all physical, visual, and scattering properties for a chemical element.
/// This struct acts as the central row for our database.
#[derive(Debug, Clone)]
struct AtomData {
    pub atomic_number: i32,
    pub symbol: &'static str,
    pub name: &'static str,

    // -- Scattering Factors --
    // Cromer-Mann coefficients: a1, b1, a2, b2, a3, b3, a4, b4, c
    pub cromer_mann: [f64; 9],

    // -- Radii (Angstroms) --
    pub covalent_radius: f64,
    pub ionic_radius: f64, // Shannon (CN=6, common ox state)
    pub vdw_radius: f64,   // Alvarez (2013)

    // -- Chemistry --
    pub electronegativity: f64, // Pauling scale

    // -- Visualization --
    pub color_rgb: (f64, f64, f64), // Material Design / CPK
}

/// The Central Database for Atomic Properties.
///
/// **Data Sources:**
/// - **Cromer-Mann**: *Int. Tab. Cryst. Vol C*, Table 6.1.1.4 (1992).
/// - **Ionic Radii**: Shannon, R.D. *Acta Cryst. A* 32, 751 (1976).
/// - **VdW Radii**: Alvarez, S. *Dalton Trans.* 42, 8617 (2013).
/// - **Colors**: Material Design Palette (Adapted for CPK-like distinctness).
/// - **Electronegativity**: Pauling, L. *The Nature of the Chemical Bond* (1960).
fn get_atom_data(element: &str) -> AtomData {
    match element {
        // --- Period 1 ---
        "H" => AtomData {
            atomic_number: 1,
            symbol: "H",
            name: "Hydrogen",
            cromer_mann: [
                0.493002, 10.5109, 0.322912, 26.1257, 0.140191, 3.14236, 0.04081, 57.7997, 0.003038,
            ],
            covalent_radius: 0.37,
            ionic_radius: 0.60,
            vdw_radius: 1.20,
            electronegativity: 2.20,
            color_rgb: (0.76, 0.88, 0.62), // Green 200
        },
        "He" => AtomData {
            atomic_number: 2,
            symbol: "He",
            name: "Helium",
            cromer_mann: [
                0.8734, 9.1037, 0.6309, 3.3568, 0.3112, 22.9276, 0.178, 0.9821, 0.0064,
            ],
            covalent_radius: 0.32,
            ionic_radius: 0.00,
            vdw_radius: 1.40,
            electronegativity: 0.00,
            color_rgb: (0.70, 0.89, 0.96), // Cyan 200
        },

        // --- Period 2 ---
        "Li" => AtomData {
            atomic_number: 3,
            symbol: "Li",
            name: "Lithium",
            cromer_mann: [
                1.1282, 3.9546, 0.7508, 1.0524, 0.6175, 85.3905, 0.4653, 168.261, 0.0377,
            ],
            covalent_radius: 1.34,
            ionic_radius: 0.76,
            vdw_radius: 1.82,
            electronegativity: 0.98,
            color_rgb: (0.94, 0.33, 0.31), // Red 400
        },
        "Be" => AtomData {
            atomic_number: 4,
            symbol: "Be",
            name: "Beryllium",
            cromer_mann: [
                1.5919, 43.6427, 1.1278, 1.8623, 0.5391, 103.483, 0.7029, 0.542, 0.0385,
            ],
            covalent_radius: 0.90,
            ionic_radius: 0.45,
            vdw_radius: 1.53,
            electronegativity: 1.57,
            color_rgb: (1.00, 0.70, 0.35), // Orange 300
        },
        "B" => AtomData {
            atomic_number: 5,
            symbol: "B",
            name: "Boron",
            cromer_mann: [
                2.0545, 23.2185, 1.3326, 1.021, 1.0979, 60.3498, 0.7068, 0.1403, -0.1932,
            ],
            covalent_radius: 0.82,
            ionic_radius: 0.27,
            vdw_radius: 1.92,
            electronegativity: 2.04,
            color_rgb: (0.30, 0.69, 0.60), // Teal 400
        },
        "C" => AtomData {
            atomic_number: 6,
            symbol: "C",
            name: "Carbon",
            cromer_mann: [
                2.31, 20.8439, 1.02, 10.2075, 1.5886, 0.5687, 0.865, 51.6512, 0.2156,
            ],
            covalent_radius: 0.77,
            ionic_radius: 0.16,
            vdw_radius: 1.70,
            electronegativity: 2.55,
            color_rgb: (0.56, 0.76, 0.29), // Green 500
        },
        "N" => AtomData {
            atomic_number: 7,
            symbol: "N",
            name: "Nitrogen",
            cromer_mann: [
                12.2126, 0.0057, 3.1322, 9.8933, 2.0125, 28.9975, 1.1663, 0.5826, -11.529,
            ],
            covalent_radius: 0.75,
            ionic_radius: 0.13,
            vdw_radius: 1.55,
            electronegativity: 3.04,
            color_rgb: (0.41, 0.73, 0.39), // Green 400
        },
        "O" => AtomData {
            atomic_number: 8,
            symbol: "O",
            name: "Oxygen",
            cromer_mann: [
                3.0485, 13.2771, 2.2868, 5.7011, 1.5463, 0.3239, 0.867, 32.9089, 0.2508,
            ],
            covalent_radius: 0.73,
            ionic_radius: 1.40,
            vdw_radius: 1.52,
            electronegativity: 3.44,
            color_rgb: (0.30, 0.69, 0.31), // Green 600
        },
        "F" => AtomData {
            atomic_number: 9,
            symbol: "F",
            name: "Fluorine",
            cromer_mann: [
                3.5392, 10.2825, 2.6412, 4.2944, 1.517, 0.2615, 1.0243, 26.1476, 0.2776,
            ],
            covalent_radius: 0.71,
            ionic_radius: 1.33,
            vdw_radius: 1.47,
            electronegativity: 3.98,
            color_rgb: (0.51, 0.78, 0.33), // Lime 500
        },
        "Ne" => AtomData {
            atomic_number: 10,
            symbol: "Ne",
            name: "Neon",
            cromer_mann: [
                3.9553, 8.4042, 3.1125, 3.4262, 1.4546, 0.2306, 1.1251, 21.7184, 0.3515,
            ],
            covalent_radius: 0.69,
            ionic_radius: 0.00,
            vdw_radius: 1.54,
            electronegativity: 0.00,
            color_rgb: (0.38, 0.80, 0.85), // Cyan 400
        },

        // --- Period 3 ---
        "Na" => AtomData {
            atomic_number: 11,
            symbol: "Na",
            name: "Sodium",
            cromer_mann: [
                4.7626, 3.285, 3.1736, 8.8422, 1.2674, 0.3136, 1.1128, 129.424, 0.676,
            ],
            covalent_radius: 1.54,
            ionic_radius: 1.02,
            vdw_radius: 2.27,
            electronegativity: 0.93,
            color_rgb: (0.92, 0.26, 0.21), // Red 500
        },
        "Mg" => AtomData {
            atomic_number: 12,
            symbol: "Mg",
            name: "Magnesium",
            cromer_mann: [
                5.4204, 2.8275, 2.1735, 79.2611, 1.2269, 0.3808, 2.3073, 7.1937, 0.8584,
            ],
            covalent_radius: 1.30,
            ionic_radius: 0.72,
            vdw_radius: 1.73,
            electronegativity: 1.31,
            color_rgb: (1.00, 0.60, 0.20), // Orange 400
        },
        "Al" => AtomData {
            atomic_number: 13,
            symbol: "Al",
            name: "Aluminium",
            cromer_mann: [
                6.4202, 3.0387, 1.9002, 0.7426, 1.5936, 31.5472, 1.9646, 85.0886, 1.1151,
            ],
            covalent_radius: 1.18,
            ionic_radius: 0.54,
            vdw_radius: 1.84,
            electronegativity: 1.61,
            color_rgb: (0.33, 0.59, 0.82), // Blue 400
        },
        "Si" => AtomData {
            atomic_number: 14,
            symbol: "Si",
            name: "Silicon",
            cromer_mann: [
                6.2915, 2.4386, 3.0353, 32.333, 1.9891, 0.6785, 1.541, 81.6937, 1.1407,
            ],
            covalent_radius: 1.11,
            ionic_radius: 0.40,
            vdw_radius: 2.10,
            electronegativity: 1.90,
            color_rgb: (0.30, 0.69, 0.60), // Teal 500
        },
        "P" => AtomData {
            atomic_number: 15,
            symbol: "P",
            name: "Phosphorus",
            cromer_mann: [
                6.4345, 1.9067, 4.1791, 27.157, 1.78, 0.526, 1.4908, 68.1645, 1.1149,
            ],
            covalent_radius: 1.06,
            ionic_radius: 0.38,
            vdw_radius: 1.80,
            electronegativity: 2.19,
            color_rgb: (0.56, 0.76, 0.29), // Green 600
        },
        "S" => AtomData {
            atomic_number: 16,
            symbol: "S",
            name: "Sulfur",
            cromer_mann: [
                6.9053, 1.4679, 5.2034, 22.2151, 1.4379, 0.2536, 1.5863, 56.172, 0.8669,
            ],
            covalent_radius: 1.02,
            ionic_radius: 1.84,
            vdw_radius: 1.80,
            electronegativity: 2.58,
            color_rgb: (0.69, 0.82, 0.24), // Lime 600
        },
        "Cl" => AtomData {
            atomic_number: 17,
            symbol: "Cl",
            name: "Chlorine",
            cromer_mann: [
                11.4604, 0.0104, 7.1964, 1.1662, 6.2556, 18.5194, 1.6455, 47.7784, -9.5574,
            ],
            covalent_radius: 0.99,
            ionic_radius: 1.81,
            vdw_radius: 1.75,
            electronegativity: 3.16,
            color_rgb: (0.51, 0.78, 0.33), // Lime 500
        },
        "Ar" => AtomData {
            atomic_number: 18,
            symbol: "Ar",
            name: "Argon",
            cromer_mann: [
                7.4845, 0.9072, 6.7723, 14.8407, 0.6539, 43.8983, 1.6442, 33.3929, 1.4445,
            ],
            covalent_radius: 0.97,
            ionic_radius: 0.00,
            vdw_radius: 1.88,
            electronegativity: 0.00,
            color_rgb: (0.18, 0.75, 0.83), // Cyan 500
        },

        // --- Period 4 ---
        "K" => AtomData {
            atomic_number: 19,
            symbol: "K",
            name: "Potassium",
            cromer_mann: [
                8.2186, 12.7949, 7.4398, 0.7748, 1.0519, 213.187, 0.8659, 41.6841, 1.4228,
            ],
            covalent_radius: 1.96,
            ionic_radius: 1.38,
            vdw_radius: 2.75,
            electronegativity: 0.82,
            color_rgb: (0.85, 0.20, 0.19), // Red 700
        },
        "Ca" => AtomData {
            atomic_number: 20,
            symbol: "Ca",
            name: "Calcium",
            cromer_mann: [
                8.6266, 10.4421, 7.3873, 0.6599, 1.5899, 85.7484, 1.0211, 178.437, 1.3751,
            ],
            covalent_radius: 1.74,
            ionic_radius: 1.00,
            vdw_radius: 2.31,
            electronegativity: 1.00,
            color_rgb: (0.96, 0.55, 0.19), // Orange 600
        },
        "Sc" => AtomData {
            atomic_number: 21,
            symbol: "Sc",
            name: "Scandium",
            cromer_mann: [
                9.189, 9.0213, 7.3679, 0.5729, 1.6409, 136.108, 1.468, 51.3531, 1.3329,
            ],
            covalent_radius: 1.44,
            ionic_radius: 0.745,
            vdw_radius: 2.30,
            electronegativity: 1.36,
            color_rgb: (0.47, 0.53, 0.60), // Blue Grey 500
        },
        "Ti" => AtomData {
            atomic_number: 22,
            symbol: "Ti",
            name: "Titanium",
            cromer_mann: [
                9.7595, 7.8508, 7.3558, 0.5, 1.6991, 35.6338, 1.9021, 116.105, 1.2807,
            ],
            covalent_radius: 1.36,
            ionic_radius: 0.605,
            vdw_radius: 2.15,
            electronegativity: 1.54,
            color_rgb: (0.55, 0.61, 0.67), // Blue Grey 400
        },
        "V" => AtomData {
            atomic_number: 23,
            symbol: "V",
            name: "Vanadium",
            cromer_mann: [
                10.2971, 6.8657, 7.3511, 0.4385, 2.0703, 26.8938, 2.0571, 102.478, 1.2199,
            ],
            covalent_radius: 1.25,
            ionic_radius: 0.59,
            vdw_radius: 2.05,
            electronegativity: 1.63,
            color_rgb: (0.38, 0.45, 0.53), // Blue Grey 600
        },
        "Cr" => AtomData {
            atomic_number: 24,
            symbol: "Cr",
            name: "Chromium",
            cromer_mann: [
                10.6406, 6.1038, 7.3537, 0.392, 3.324, 20.2626, 1.4922, 98.7399, 1.1832,
            ],
            covalent_radius: 1.27,
            ionic_radius: 0.615,
            vdw_radius: 2.05,
            electronegativity: 1.66,
            color_rgb: (0.33, 0.39, 0.45), // Blue Grey 700
        },
        "Mn" => AtomData {
            atomic_number: 25,
            symbol: "Mn",
            name: "Manganese",
            cromer_mann: [
                11.2819, 5.3409, 7.3573, 0.3432, 3.0193, 17.8674, 2.2441, 83.7543, 1.0896,
            ],
            covalent_radius: 1.39,
            ionic_radius: 0.83,
            vdw_radius: 2.05,
            electronegativity: 1.55,
            color_rgb: (0.26, 0.32, 0.36), // Blue Grey 800
        },
        "Fe" => AtomData {
            atomic_number: 26,
            symbol: "Fe",
            name: "Iron",
            cromer_mann: [
                11.7695, 4.7611, 7.3573, 0.3072, 3.5222, 15.3535, 2.3045, 76.8805, 1.0369,
            ],
            covalent_radius: 1.25,
            ionic_radius: 0.78,
            vdw_radius: 2.00,
            electronegativity: 1.83,
            color_rgb: (0.21, 0.27, 0.31), // Blue Grey 900
        },
        "Co" => AtomData {
            atomic_number: 27,
            symbol: "Co",
            name: "Cobalt",
            cromer_mann: [
                12.2841, 4.2791, 7.3409, 0.2784, 4.0034, 13.5359, 2.3488, 71.1692, 1.0118,
            ],
            covalent_radius: 1.26,
            ionic_radius: 0.745,
            vdw_radius: 2.00,
            electronegativity: 1.88,
            color_rgb: (0.38, 0.45, 0.53),
        },
        "Ni" => AtomData {
            atomic_number: 28,
            symbol: "Ni",
            name: "Nickel",
            cromer_mann: [
                12.8376, 3.8785, 7.292, 0.2565, 4.4438, 12.1763, 2.38, 66.3421, 1.0341,
            ],
            covalent_radius: 1.21,
            ionic_radius: 0.69,
            vdw_radius: 1.97,
            electronegativity: 1.91,
            color_rgb: (0.47, 0.53, 0.60),
        },
        "Cu" => AtomData {
            atomic_number: 29,
            symbol: "Cu",
            name: "Copper",
            cromer_mann: [
                13.338, 3.5828, 7.1676, 0.247, 5.6158, 11.3966, 1.6735, 64.8126, 1.191,
            ],
            covalent_radius: 1.38,
            ionic_radius: 0.73,
            vdw_radius: 1.96,
            electronegativity: 1.90,
            color_rgb: (0.55, 0.61, 0.67),
        },
        "Zn" => AtomData {
            atomic_number: 30,
            symbol: "Zn",
            name: "Zinc",
            cromer_mann: [
                14.0743, 3.2655, 7.0318, 0.2333, 5.1652, 10.3163, 2.41, 58.7097, 1.3041,
            ],
            covalent_radius: 1.31,
            ionic_radius: 0.74,
            vdw_radius: 2.01,
            electronegativity: 1.65,
            color_rgb: (0.69, 0.75, 0.78),
        },
        "Ga" => AtomData {
            atomic_number: 31,
            symbol: "Ga",
            name: "Gallium",
            cromer_mann: [
                15.2354, 3.0669, 6.7006, 0.2412, 4.3591, 10.7805, 2.9623, 61.4135, 1.7189,
            ],
            covalent_radius: 1.26,
            ionic_radius: 0.62,
            vdw_radius: 1.87,
            electronegativity: 1.81,
            color_rgb: (0.33, 0.59, 0.82),
        },
        "Ge" => AtomData {
            atomic_number: 32,
            symbol: "Ge",
            name: "Germanium",
            cromer_mann: [
                16.0816, 2.8509, 6.3747, 0.2516, 3.7068, 11.4468, 3.683, 54.7625, 2.1313,
            ],
            covalent_radius: 1.22,
            ionic_radius: 0.53,
            vdw_radius: 2.11,
            electronegativity: 2.01,
            color_rgb: (0.30, 0.69, 0.60),
        },
        "As" => AtomData {
            atomic_number: 33,
            symbol: "As",
            name: "Arsenic",
            cromer_mann: [
                16.6723, 2.6345, 6.0701, 0.2647, 3.4313, 12.9479, 4.2779, 47.7972, 2.531,
            ],
            covalent_radius: 1.19,
            ionic_radius: 0.58,
            vdw_radius: 1.85,
            electronegativity: 2.18,
            color_rgb: (0.30, 0.69, 0.60),
        },
        "Se" => AtomData {
            atomic_number: 34,
            symbol: "Se",
            name: "Selenium",
            cromer_mann: [
                17.0006, 2.4098, 5.8196, 0.2726, 3.9731, 15.2372, 4.3543, 43.8163, 2.8409,
            ],
            covalent_radius: 1.16,
            ionic_radius: 1.98,
            vdw_radius: 1.90,
            electronegativity: 2.55,
            color_rgb: (0.56, 0.76, 0.29),
        },
        "Br" => AtomData {
            atomic_number: 35,
            symbol: "Br",
            name: "Bromine",
            cromer_mann: [
                17.1789, 2.1723, 5.2358, 16.5796, 5.6377, 0.2609, 3.9851, 41.4328, 2.9557,
            ],
            covalent_radius: 1.14,
            ionic_radius: 1.96,
            vdw_radius: 1.85,
            electronegativity: 2.96,
            color_rgb: (0.69, 0.82, 0.24),
        },
        "Kr" => AtomData {
            atomic_number: 36,
            symbol: "Kr",
            name: "Krypton",
            cromer_mann: [
                17.3555, 1.9384, 6.7286, 16.5623, 5.5493, 0.2261, 3.5375, 39.3972, 2.825,
            ],
            covalent_radius: 1.10,
            ionic_radius: 0.00,
            vdw_radius: 2.02,
            electronegativity: 3.00,
            color_rgb: (0.18, 0.75, 0.83),
        },

        // --- Period 5 ---
        "Rb" => AtomData {
            atomic_number: 37,
            symbol: "Rb",
            name: "Rubidium",
            cromer_mann: [
                17.1784, 1.7888, 9.6435, 17.3151, 5.1399, 0.2748, 1.5292, 164.934, 3.4873,
            ],
            covalent_radius: 2.11,
            ionic_radius: 1.52,
            vdw_radius: 3.03,
            electronegativity: 0.82,
            color_rgb: (0.78, 0.17, 0.16), // Red 800
        },
        "Sr" => AtomData {
            atomic_number: 38,
            symbol: "Sr",
            name: "Strontium",
            cromer_mann: [
                17.5663, 1.5564, 9.8184, 14.0988, 5.422, 0.1664, 2.6694, 132.376, 2.5064,
            ],
            covalent_radius: 1.92,
            ionic_radius: 1.18,
            vdw_radius: 2.49,
            electronegativity: 0.95,
            color_rgb: (0.94, 0.50, 0.20),
        },
        "Y" => AtomData {
            atomic_number: 39,
            symbol: "Y",
            name: "Yttrium",
            cromer_mann: [
                17.776, 1.4029, 10.2946, 12.8006, 5.72629, 0.125599, 3.26588, 104.354, 1.91213,
            ],
            covalent_radius: 1.62,
            ionic_radius: 0.90,
            vdw_radius: 2.40,
            electronegativity: 1.22,
            color_rgb: (0.47, 0.53, 0.60),
        },
        "Zr" => AtomData {
            atomic_number: 40,
            symbol: "Zr",
            name: "Zirconium",
            cromer_mann: [
                17.8765, 1.27618, 10.948, 11.916, 5.41732, 0.117622, 3.65721, 87.6627, 2.06929,
            ],
            covalent_radius: 1.48,
            ionic_radius: 0.72,
            vdw_radius: 2.30,
            electronegativity: 1.33,
            color_rgb: (0.55, 0.61, 0.67),
        },
        "Nb" => AtomData {
            atomic_number: 41,
            symbol: "Nb",
            name: "Niobium",
            cromer_mann: [
                17.6142, 1.18865, 12.0144, 11.766, 4.04183, 0.204785, 3.53346, 69.7957, 3.75591,
            ],
            covalent_radius: 1.37,
            ionic_radius: 0.64,
            vdw_radius: 2.15,
            electronegativity: 1.60,
            color_rgb: (0.38, 0.45, 0.53),
        },
        "Mo" => AtomData {
            atomic_number: 42,
            symbol: "Mo",
            name: "Molybdenum",
            cromer_mann: [
                3.7025, 0.2772, 17.2356, 1.0958, 12.8876, 11.004, 3.7429, 61.6584, 4.3875,
            ],
            covalent_radius: 1.45,
            ionic_radius: 0.59,
            vdw_radius: 2.10,
            electronegativity: 2.16,
            color_rgb: (0.33, 0.39, 0.45),
        },
        "Tc" => AtomData {
            atomic_number: 43,
            symbol: "Tc",
            name: "Technetium",
            cromer_mann: [
                19.1301, 0.864132, 11.0948, 8.14487, 4.64901, 21.5707, 2.71263, 86.8472, 5.40428,
            ],
            covalent_radius: 1.56,
            ionic_radius: 0.56,
            vdw_radius: 2.05,
            electronegativity: 1.90,
            color_rgb: (0.26, 0.32, 0.36),
        },
        "Ru" => AtomData {
            atomic_number: 44,
            symbol: "Ru",
            name: "Ruthenium",
            cromer_mann: [
                19.2674, 0.80852, 12.9182, 8.43467, 4.86337, 24.7997, 1.56756, 94.2928, 5.37874,
            ],
            covalent_radius: 1.26,
            ionic_radius: 0.62,
            vdw_radius: 2.05,
            electronegativity: 2.20,
            color_rgb: (0.21, 0.27, 0.31),
        },
        "Rh" => AtomData {
            atomic_number: 45,
            symbol: "Rh",
            name: "Rhodium",
            cromer_mann: [
                19.2957, 0.751536, 14.3501, 8.21758, 4.73425, 25.8749, 1.28918, 98.6062, 5.328,
            ],
            covalent_radius: 1.35,
            ionic_radius: 0.665,
            vdw_radius: 2.00,
            electronegativity: 2.28,
            color_rgb: (0.38, 0.45, 0.53),
        },
        "Pd" => AtomData {
            atomic_number: 46,
            symbol: "Pd",
            name: "Palladium",
            cromer_mann: [
                19.3319, 0.698655, 15.5017, 7.98929, 5.29537, 25.2052, 0.605844, 76.8986, 5.26593,
            ],
            covalent_radius: 1.31,
            ionic_radius: 0.86,
            vdw_radius: 2.05,
            electronegativity: 2.20,
            color_rgb: (0.47, 0.53, 0.60),
        },
        "Ag" => AtomData {
            atomic_number: 47,
            symbol: "Ag",
            name: "Silver",
            cromer_mann: [
                19.2808, 0.6446, 16.6885, 7.4726, 4.8045, 24.6605, 1.0463, 99.8156, 5.179,
            ],
            covalent_radius: 1.53,
            ionic_radius: 1.15,
            vdw_radius: 2.03,
            electronegativity: 1.93,
            color_rgb: (0.69, 0.75, 0.78),
        },
        "Cd" => AtomData {
            atomic_number: 48,
            symbol: "Cd",
            name: "Cadmium",
            cromer_mann: [
                19.2214, 0.5946, 17.6444, 6.9089, 4.461, 24.7008, 1.6029, 87.4825, 5.0694,
            ],
            covalent_radius: 1.48,
            ionic_radius: 0.95,
            vdw_radius: 2.18,
            electronegativity: 1.69,
            color_rgb: (0.33, 0.59, 0.82),
        },
        "In" => AtomData {
            atomic_number: 49,
            symbol: "In",
            name: "Indium",
            cromer_mann: [
                19.1624, 0.5476, 18.5596, 6.3776, 4.2948, 25.8499, 2.0396, 92.8029, 4.9391,
            ],
            covalent_radius: 1.44,
            ionic_radius: 0.80,
            vdw_radius: 1.93,
            electronegativity: 1.78,
            color_rgb: (0.33, 0.59, 0.82),
        },
        "Sn" => AtomData {
            atomic_number: 50,
            symbol: "Sn",
            name: "Tin",
            cromer_mann: [
                19.1889, 5.8303, 19.1005, 0.5031, 4.4585, 26.8909, 2.4663, 83.9571, 4.7821,
            ],
            covalent_radius: 1.41,
            ionic_radius: 0.69,
            vdw_radius: 2.17,
            electronegativity: 1.96,
            color_rgb: (0.30, 0.69, 0.60),
        },
        "Sb" => AtomData {
            atomic_number: 51,
            symbol: "Sb",
            name: "Antimony",
            cromer_mann: [
                19.6418, 5.3034, 19.0455, 0.4607, 5.0371, 27.9074, 2.6827, 75.2825, 4.5909,
            ],
            covalent_radius: 1.38,
            ionic_radius: 0.76,
            vdw_radius: 2.06,
            electronegativity: 2.05,
            color_rgb: (0.30, 0.69, 0.60),
        },
        "Te" => AtomData {
            atomic_number: 52,
            symbol: "Te",
            name: "Tellurium",
            cromer_mann: [
                19.9644, 4.81742, 19.0138, 0.420885, 6.14487, 28.5284, 2.5239, 70.8403, 4.352,
            ],
            covalent_radius: 1.35,
            ionic_radius: 2.21,
            vdw_radius: 2.06,
            electronegativity: 2.10,
            color_rgb: (0.56, 0.76, 0.29),
        },
        "I" => AtomData {
            atomic_number: 53,
            symbol: "I",
            name: "Iodine",
            cromer_mann: [
                20.1472, 4.347, 18.9949, 0.3814, 7.5138, 27.766, 2.2735, 66.8776, 4.0712,
            ],
            covalent_radius: 1.33,
            ionic_radius: 2.20,
            vdw_radius: 1.98,
            electronegativity: 2.66,
            color_rgb: (0.69, 0.82, 0.24),
        },
        "Xe" => AtomData {
            atomic_number: 54,
            symbol: "Xe",
            name: "Xenon",
            cromer_mann: [
                20.2933, 3.9282, 19.0298, 0.344, 8.9767, 26.4659, 1.99, 64.2658, 3.7118,
            ],
            covalent_radius: 1.30,
            ionic_radius: 0.00,
            vdw_radius: 2.16,
            electronegativity: 2.60,
            color_rgb: (0.18, 0.75, 0.83),
        },

        // --- Period 6 ---
        "Cs" => AtomData {
            atomic_number: 55,
            symbol: "Cs",
            name: "Caesium",
            cromer_mann: [
                20.3892, 3.569, 19.1062, 0.3107, 10.662, 24.3879, 1.4953, 213.904, 3.3352,
            ],
            covalent_radius: 2.25,
            ionic_radius: 1.67,
            vdw_radius: 3.43,
            electronegativity: 0.79,
            color_rgb: (0.72, 0.11, 0.11), // Red 900
        },
        "Ba" => AtomData {
            atomic_number: 56,
            symbol: "Ba",
            name: "Barium",
            cromer_mann: [
                20.3361, 3.216, 19.297, 0.2756, 10.888, 20.2073, 2.69599, 167.202, 2.7731,
            ],
            covalent_radius: 1.98,
            ionic_radius: 1.35,
            vdw_radius: 2.68,
            electronegativity: 0.89,
            color_rgb: (0.90, 0.49, 0.13),
        },
        // Lanthanides
        "La" => AtomData {
            atomic_number: 57,
            symbol: "La",
            name: "Lanthanum",
            cromer_mann: [
                20.578, 2.94817, 19.599, 0.244475, 11.3727, 18.7726, 3.28719, 133.124, 2.14678,
            ],
            covalent_radius: 1.69,
            ionic_radius: 1.03,
            vdw_radius: 2.50,
            electronegativity: 1.10,
            color_rgb: (0.58, 0.46, 0.80), // Deep Purple
        },
        "Ce" => AtomData {
            atomic_number: 58,
            symbol: "Ce",
            name: "Cerium",
            cromer_mann: [
                21.1671, 2.81219, 19.7695, 0.226836, 11.8513, 17.6083, 3.33049, 127.113, 1.86264,
            ],
            covalent_radius: 1.63,
            ionic_radius: 1.01,
            vdw_radius: 2.48,
            electronegativity: 1.12,
            color_rgb: (0.49, 0.34, 0.76),
        },
        "Pr" => AtomData {
            atomic_number: 59,
            symbol: "Pr",
            name: "Praseodymium",
            cromer_mann: [
                22.044, 2.77393, 19.6697, 0.222087, 12.3856, 16.7669, 2.82428, 143.644, 2.0583,
            ],
            covalent_radius: 1.76,
            ionic_radius: 0.99,
            vdw_radius: 2.47,
            electronegativity: 1.13,
            color_rgb: (0.40, 0.28, 0.71),
        },
        "Nd" => AtomData {
            atomic_number: 60,
            symbol: "Nd",
            name: "Neodymium",
            cromer_mann: [
                22.6845, 2.66248, 19.6847, 0.210628, 12.774, 15.885, 2.85137, 137.903, 1.98486,
            ],
            covalent_radius: 1.74,
            ionic_radius: 0.98,
            vdw_radius: 2.45,
            electronegativity: 1.14,
            color_rgb: (0.31, 0.23, 0.64),
        },
        "Pm" => AtomData {
            atomic_number: 61,
            symbol: "Pm",
            name: "Promethium",
            cromer_mann: [
                23.3405, 2.5627, 19.6095, 0.202088, 13.1235, 15.1009, 2.87516, 132.721, 2.02876,
            ],
            covalent_radius: 1.73,
            ionic_radius: 0.97,
            vdw_radius: 2.43,
            electronegativity: 1.13,
            color_rgb: (0.26, 0.20, 0.59),
        },
        "Sm" => AtomData {
            atomic_number: 62,
            symbol: "Sm",
            name: "Samarium",
            cromer_mann: [
                24.0042, 2.47274, 19.4258, 0.196451, 13.4396, 14.3996, 2.89604, 128.007, 2.20963,
            ],
            covalent_radius: 1.72,
            ionic_radius: 0.96,
            vdw_radius: 2.42,
            electronegativity: 1.17,
            color_rgb: (0.21, 0.18, 0.53),
        },
        "Eu" => AtomData {
            atomic_number: 63,
            symbol: "Eu",
            name: "Europium",
            cromer_mann: [
                24.6274, 2.3879, 19.0886, 0.1942, 13.7603, 13.7546, 2.9227, 123.174, 2.5745,
            ],
            covalent_radius: 1.68,
            ionic_radius: 1.09,
            vdw_radius: 2.40,
            electronegativity: 1.20,
            color_rgb: (0.18, 0.16, 0.49),
        },
        "Gd" => AtomData {
            atomic_number: 64,
            symbol: "Gd",
            name: "Gadolinium",
            cromer_mann: [
                25.0709, 2.25341, 19.0798, 0.181951, 13.8518, 12.9331, 3.54545, 101.398, 2.4196,
            ],
            covalent_radius: 1.69,
            ionic_radius: 0.94,
            vdw_radius: 2.38,
            electronegativity: 1.20,
            color_rgb: (0.16, 0.14, 0.45),
        },
        "Tb" => AtomData {
            atomic_number: 65,
            symbol: "Tb",
            name: "Terbium",
            cromer_mann: [
                25.8976, 2.24256, 18.2185, 0.196143, 14.3167, 12.6648, 2.95354, 115.362, 3.58324,
            ],
            covalent_radius: 1.68,
            ionic_radius: 0.92,
            vdw_radius: 2.37,
            electronegativity: 1.20,
            color_rgb: (0.14, 0.12, 0.41),
        },
        "Dy" => AtomData {
            atomic_number: 66,
            symbol: "Dy",
            name: "Dysprosium",
            cromer_mann: [
                26.507, 2.1802, 17.6383, 0.202172, 14.5596, 12.1899, 2.96577, 111.874, 4.29728,
            ],
            covalent_radius: 1.67,
            ionic_radius: 0.91,
            vdw_radius: 2.35,
            electronegativity: 1.22,
            color_rgb: (0.12, 0.10, 0.37),
        },
        "Ho" => AtomData {
            atomic_number: 67,
            symbol: "Ho",
            name: "Holmium",
            cromer_mann: [
                26.9049, 2.07051, 17.294, 0.19794, 14.5583, 11.4407, 3.63837, 92.6566, 4.56796,
            ],
            covalent_radius: 1.66,
            ionic_radius: 0.90,
            vdw_radius: 2.33,
            electronegativity: 1.23,
            color_rgb: (0.10, 0.09, 0.34),
        },
        "Er" => AtomData {
            atomic_number: 68,
            symbol: "Er",
            name: "Erbium",
            cromer_mann: [
                27.6563, 2.07356, 16.4285, 0.223545, 14.9779, 11.3604, 2.98233, 105.703, 5.92046,
            ],
            covalent_radius: 1.65,
            ionic_radius: 0.89,
            vdw_radius: 2.32,
            electronegativity: 1.24,
            color_rgb: (0.09, 0.08, 0.31),
        },
        "Tm" => AtomData {
            atomic_number: 69,
            symbol: "Tm",
            name: "Thulium",
            cromer_mann: [
                28.1819, 2.02859, 15.8851, 0.238849, 15.1542, 10.9975, 2.98706, 102.961, 6.75621,
            ],
            covalent_radius: 1.64,
            ionic_radius: 0.88,
            vdw_radius: 2.30,
            electronegativity: 1.25,
            color_rgb: (0.08, 0.07, 0.28),
        },
        "Yb" => AtomData {
            atomic_number: 70,
            symbol: "Yb",
            name: "Ytterbium",
            cromer_mann: [
                28.6641, 1.9889, 15.4345, 0.257119, 15.3087, 10.6647, 2.98963, 100.417, 7.56672,
            ],
            covalent_radius: 1.63,
            ionic_radius: 0.86,
            vdw_radius: 2.28,
            electronegativity: 1.10,
            color_rgb: (0.07, 0.06, 0.25),
        },
        "Lu" => AtomData {
            atomic_number: 71,
            symbol: "Lu",
            name: "Lutetium",
            cromer_mann: [
                28.9476, 1.90182, 15.2208, 9.98519, 15.1, 0.261033, 3.71601, 84.3298, 7.97628,
            ],
            covalent_radius: 1.62,
            ionic_radius: 0.85,
            vdw_radius: 2.27,
            electronegativity: 1.27,
            color_rgb: (0.06, 0.05, 0.22),
        },
        "Hf" => AtomData {
            atomic_number: 72,
            symbol: "Hf",
            name: "Hafnium",
            cromer_mann: [
                29.144, 1.83262, 15.1726, 9.5999, 14.7586, 0.275116, 4.30013, 72.029, 8.58154,
            ],
            covalent_radius: 1.52,
            ionic_radius: 0.71,
            vdw_radius: 2.25,
            electronegativity: 1.30,
            color_rgb: (0.29, 0.35, 0.38),
        },
        "Ta" => AtomData {
            atomic_number: 73,
            symbol: "Ta",
            name: "Tantalum",
            cromer_mann: [
                29.2024, 1.77333, 15.2293, 9.37046, 14.5135, 0.295977, 4.76492, 63.3644, 9.24354,
            ],
            covalent_radius: 1.46,
            ionic_radius: 0.64,
            vdw_radius: 2.20,
            electronegativity: 1.50,
            color_rgb: (0.29, 0.35, 0.38),
        },
        "W" => AtomData {
            atomic_number: 74,
            symbol: "W",
            name: "Tungsten",
            cromer_mann: [
                29.0818, 1.72029, 15.43, 9.2259, 14.4327, 0.321703, 5.11982, 57.056, 9.8875,
            ],
            covalent_radius: 1.37,
            ionic_radius: 0.60,
            vdw_radius: 2.10,
            electronegativity: 2.36,
            color_rgb: (0.29, 0.35, 0.38),
        },
        "Re" => AtomData {
            atomic_number: 75,
            symbol: "Re",
            name: "Rhenium",
            cromer_mann: [
                28.7621, 1.67191, 15.7189, 9.09227, 14.5564, 0.3505, 5.44174, 52.0861, 10.472,
            ],
            covalent_radius: 1.31,
            ionic_radius: 0.63,
            vdw_radius: 2.05,
            electronegativity: 1.90,
            color_rgb: (0.29, 0.35, 0.38),
        },
        "Os" => AtomData {
            atomic_number: 76,
            symbol: "Os",
            name: "Osmium",
            cromer_mann: [
                28.1894, 1.62903, 16.155, 8.97948, 14.9305, 0.382661, 5.67589, 48.1647, 11.0005,
            ],
            covalent_radius: 1.29,
            ionic_radius: 0.63,
            vdw_radius: 2.00,
            electronegativity: 2.20,
            color_rgb: (0.29, 0.35, 0.38),
        },
        "Ir" => AtomData {
            atomic_number: 77,
            symbol: "Ir",
            name: "Iridium",
            cromer_mann: [
                27.3049, 1.59279, 16.7296, 8.86553, 15.6115, 0.417916, 5.83377, 45.0011, 11.4722,
            ],
            covalent_radius: 1.22,
            ionic_radius: 0.68,
            vdw_radius: 2.00,
            electronegativity: 2.20,
            color_rgb: (0.29, 0.35, 0.38),
        },
        "Pt" => AtomData {
            atomic_number: 78,
            symbol: "Pt",
            name: "Platinum",
            cromer_mann: [
                27.0059, 1.51293, 17.7639, 8.81174, 15.7131, 0.424593, 5.7837, 38.6103, 11.6883,
            ],
            covalent_radius: 1.23,
            ionic_radius: 0.86,
            vdw_radius: 2.05,
            electronegativity: 2.28,
            color_rgb: (0.29, 0.35, 0.38),
        },
        "Au" => AtomData {
            atomic_number: 79,
            symbol: "Au",
            name: "Gold",
            cromer_mann: [
                16.8819, 0.4611, 18.5913, 8.6216, 25.5582, 1.4826, 5.86, 36.3956, 12.0658,
            ],
            covalent_radius: 1.24,
            ionic_radius: 1.37,
            vdw_radius: 2.10,
            electronegativity: 2.54,
            color_rgb: (1.00, 0.82, 0.14), // Gold
        },
        "Hg" => AtomData {
            atomic_number: 80,
            symbol: "Hg",
            name: "Mercury",
            cromer_mann: [
                20.6809, 0.545, 19.0417, 8.4484, 21.6575, 1.5729, 5.9676, 38.3246, 12.6089,
            ],
            covalent_radius: 1.33,
            ionic_radius: 1.02,
            vdw_radius: 2.05,
            electronegativity: 2.00,
            color_rgb: (0.72, 0.72, 0.73),
        },
        "Tl" => AtomData {
            atomic_number: 81,
            symbol: "Tl",
            name: "Thallium",
            cromer_mann: [
                27.5446, 0.65515, 19.1584, 8.70751, 15.538, 1.96347, 5.52593, 45.8149, 13.1746,
            ],
            covalent_radius: 1.44,
            ionic_radius: 1.50,
            vdw_radius: 1.96,
            electronegativity: 1.62,
            color_rgb: (0.65, 0.33, 0.33),
        },
        "Pb" => AtomData {
            atomic_number: 82,
            symbol: "Pb",
            name: "Lead",
            cromer_mann: [
                31.0617, 0.6902, 13.0637, 2.3576, 18.442, 8.618, 5.9696, 47.2579, 13.4118,
            ],
            covalent_radius: 1.44,
            ionic_radius: 1.19,
            vdw_radius: 2.02,
            electronegativity: 2.33,
            color_rgb: (0.34, 0.35, 0.38),
        },
        "Bi" => AtomData {
            atomic_number: 83,
            symbol: "Bi",
            name: "Bismuth",
            cromer_mann: [
                33.3689, 0.704, 12.951, 2.9238, 16.5877, 8.7937, 6.4692, 48.0093, 13.5782,
            ],
            covalent_radius: 1.51,
            ionic_radius: 1.03,
            vdw_radius: 2.07,
            electronegativity: 2.02,
            color_rgb: (0.62, 0.31, 0.71),
        },
        "Po" => AtomData {
            atomic_number: 84,
            symbol: "Po",
            name: "Polonium",
            cromer_mann: [
                34.6726, 0.700999, 15.4733, 3.55078, 13.1138, 9.55642, 7.02588, 47.0045, 13.677,
            ],
            covalent_radius: 1.45,
            ionic_radius: 0.94,
            vdw_radius: 1.97,
            electronegativity: 2.00,
            color_rgb: (0.67, 0.33, 0.00),
        },
        "At" => AtomData {
            atomic_number: 85,
            symbol: "At",
            name: "Astatine",
            cromer_mann: [
                35.3163, 0.68587, 19.0211, 3.97458, 9.49887, 11.3824, 7.42518, 45.4715, 13.7108,
            ],
            covalent_radius: 1.47,
            ionic_radius: 0.62,
            vdw_radius: 2.02,
            electronegativity: 2.20,
            color_rgb: (0.46, 0.31, 0.27),
        },
        "Rn" => AtomData {
            atomic_number: 86,
            symbol: "Rn",
            name: "Radon",
            cromer_mann: [
                35.5631, 0.6631, 21.2816, 4.0691, 8.0037, 14.0422, 7.4433, 44.2473, 13.6905,
            ],
            covalent_radius: 1.42,
            ionic_radius: 0.00,
            vdw_radius: 2.20,
            electronegativity: 2.20,
            color_rgb: (0.26, 0.51, 0.59),
        },

        // --- Period 7 ---
        "Fr" => AtomData {
            atomic_number: 87,
            symbol: "Fr",
            name: "Francium",
            cromer_mann: [
                35.9299, 0.646453, 23.0547, 4.17619, 12.1439, 23.1052, 2.11253, 150.645, 13.7247,
            ],
            covalent_radius: 2.60,
            ionic_radius: 1.80,
            vdw_radius: 3.48,
            electronegativity: 0.70,
            color_rgb: (0.62, 0.08, 0.08),
        },
        "Ra" => AtomData {
            atomic_number: 88,
            symbol: "Ra",
            name: "Radium",
            cromer_mann: [
                35.763, 0.616341, 22.9064, 3.87135, 12.4739, 19.9887, 3.21097, 142.325, 13.6211,
            ],
            covalent_radius: 2.21,
            ionic_radius: 1.48,
            vdw_radius: 2.83,
            electronegativity: 0.90,
            color_rgb: (0.85, 0.40, 0.11),
        },
        "Ac" => AtomData {
            atomic_number: 89,
            symbol: "Ac",
            name: "Actinium",
            cromer_mann: [
                35.6597, 0.589092, 23.1032, 3.65155, 12.5977, 18.599, 4.08655, 117.02, 13.5266,
            ],
            covalent_radius: 2.15,
            ionic_radius: 1.12,
            vdw_radius: 2.00,
            electronegativity: 1.10,
            color_rgb: (0.49, 0.54, 0.80),
        },
        "Th" => AtomData {
            atomic_number: 90,
            symbol: "Th",
            name: "Thorium",
            cromer_mann: [
                35.5645, 0.563359, 23.4219, 3.46204, 12.7473, 17.8309, 4.80703, 99.1722, 13.4314,
            ],
            covalent_radius: 2.06,
            ionic_radius: 1.05,
            vdw_radius: 2.40,
            electronegativity: 1.30,
            color_rgb: (0.39, 0.44, 0.75),
        },
        "Pa" => AtomData {
            atomic_number: 91,
            symbol: "Pa",
            name: "Protactinium",
            cromer_mann: [
                35.8847, 0.547751, 23.2948, 3.41519, 14.1891, 16.9235, 4.17287, 105.251, 13.4287,
            ],
            covalent_radius: 2.00,
            ionic_radius: 0.99,
            vdw_radius: 2.00,
            electronegativity: 1.50,
            color_rgb: (0.30, 0.34, 0.71),
        },
        "U" => AtomData {
            atomic_number: 92,
            symbol: "U",
            name: "Uranium",
            cromer_mann: [
                36.0228, 0.5293, 23.4128, 3.3253, 14.9491, 16.0927, 4.188, 100.613, 13.3966,
            ],
            covalent_radius: 1.96,
            ionic_radius: 1.00,
            vdw_radius: 1.86,
            electronegativity: 1.38,
            color_rgb: (0.25, 0.30, 0.67),
        },
        "Np" => AtomData {
            atomic_number: 93,
            symbol: "Np",
            name: "Neptunium",
            cromer_mann: [
                36.1874, 0.511929, 23.5964, 3.25396, 15.6402, 15.3622, 4.1855, 97.4908, 13.3573,
            ],
            covalent_radius: 1.90,
            ionic_radius: 0.98,
            vdw_radius: 2.00,
            electronegativity: 1.36,
            color_rgb: (0.21, 0.26, 0.63),
        },
        "Pu" => AtomData {
            atomic_number: 94,
            symbol: "Pu",
            name: "Plutonium",
            cromer_mann: [
                35.5103, 0.498626, 22.5787, 2.96627, 12.7766, 11.9484, 4.92159, 22.7502, 13.2116,
            ],
            covalent_radius: 1.87,
            ionic_radius: 0.96,
            vdw_radius: 2.00,
            electronegativity: 1.28,
            color_rgb: (0.18, 0.23, 0.59),
        },
        "Am" => AtomData {
            atomic_number: 95,
            symbol: "Am",
            name: "Americium",
            cromer_mann: [
                36.6706, 0.483629, 24.0992, 3.20647, 17.3415, 14.3136, 3.49331, 102.273, 13.3592,
            ],
            covalent_radius: 1.80,
            ionic_radius: 0.95,
            vdw_radius: 2.00,
            electronegativity: 1.13,
            color_rgb: (0.16, 0.20, 0.55),
        },
        "Cm" => AtomData {
            atomic_number: 96,
            symbol: "Cm",
            name: "Curium",
            cromer_mann: [
                36.6488, 0.465154, 24.4096, 3.08997, 17.399, 13.4346, 4.21665, 88.4834, 13.2887,
            ],
            covalent_radius: 1.69,
            ionic_radius: 0.94,
            vdw_radius: 2.00,
            electronegativity: 1.28,
            color_rgb: (0.14, 0.18, 0.51),
        },
        "Bk" => AtomData {
            atomic_number: 97,
            symbol: "Bk",
            name: "Berkelium",
            cromer_mann: [
                36.7881, 0.451018, 24.7736, 3.04619, 17.8919, 12.8946, 4.23284, 86.003, 13.2754,
            ],
            covalent_radius: 1.66,
            ionic_radius: 0.93,
            vdw_radius: 2.00,
            electronegativity: 1.30,
            color_rgb: (0.12, 0.16, 0.47),
        },
        "Cf" => AtomData {
            atomic_number: 98,
            symbol: "Cf",
            name: "Californium",
            cromer_mann: [
                36.9185, 0.437533, 25.1995, 3.00775, 18.3317, 12.4044, 4.24391, 83.7881, 13.2674,
            ],
            covalent_radius: 1.63,
            ionic_radius: 0.92,
            vdw_radius: 2.00,
            electronegativity: 1.30,
            color_rgb: (0.10, 0.14, 0.43),
        },
        "Es" => AtomData {
            atomic_number: 99,
            symbol: "Es",
            name: "Einsteinium",
            cromer_mann: [
                2.31, 20.8439, 1.02, 10.2075, 1.5886, 0.5687, 0.865, 51.6512, 0.2156,
            ], // Dummy
            covalent_radius: 1.62,
            ionic_radius: 0.91,
            vdw_radius: 2.00,
            electronegativity: 1.30,
            color_rgb: (0.09, 0.12, 0.39),
        },
        "Fm" => AtomData {
            atomic_number: 100,
            symbol: "Fm",
            name: "Fermium",
            cromer_mann: [
                2.31, 20.8439, 1.02, 10.2075, 1.5886, 0.5687, 0.865, 51.6512, 0.2156,
            ], // Dummy
            covalent_radius: 1.61,
            ionic_radius: 0.90,
            vdw_radius: 2.00,
            electronegativity: 1.30,
            color_rgb: (0.08, 0.11, 0.35),
        },
        "Md" => AtomData {
            atomic_number: 101,
            symbol: "Md",
            name: "Mendelevium",
            cromer_mann: [
                2.31, 20.8439, 1.02, 10.2075, 1.5886, 0.5687, 0.865, 51.6512, 0.2156,
            ], // Dummy
            covalent_radius: 1.60,
            ionic_radius: 0.89,
            vdw_radius: 2.00,
            electronegativity: 1.30,
            color_rgb: (0.07, 0.10, 0.32),
        },
        "No" => AtomData {
            atomic_number: 102,
            symbol: "No",
            name: "Nobelium",
            cromer_mann: [
                2.31, 20.8439, 1.02, 10.2075, 1.5886, 0.5687, 0.865, 51.6512, 0.2156,
            ], // Dummy
            covalent_radius: 1.59,
            ionic_radius: 0.88,
            vdw_radius: 2.00,
            electronegativity: 1.30,
            color_rgb: (0.06, 0.09, 0.29),
        },
        "Lr" => AtomData {
            atomic_number: 103,
            symbol: "Lr",
            name: "Lawrencium",
            cromer_mann: [
                2.31, 20.8439, 1.02, 10.2075, 1.5886, 0.5687, 0.865, 51.6512, 0.2156,
            ], // Dummy
            covalent_radius: 1.58,
            ionic_radius: 0.87,
            vdw_radius: 2.00,
            electronegativity: 1.30,
            color_rgb: (0.05, 0.08, 0.26),
        },

        // --- Unknown / Default ---
        _ => AtomData {
            atomic_number: 0,
            symbol: "Xx",
            name: "Unknown",
            cromer_mann: [
                2.31, 20.8439, 1.02, 10.2075, 1.5886, 0.5687, 0.865, 51.6512, 0.2156,
            ], // Default Carbon
            covalent_radius: 1.50,
            ionic_radius: 0.00,
            vdw_radius: 2.00,
            electronegativity: 0.00,
            color_rgb: (1.00, 0.08, 0.58), // Hot Pink
        },
    }
}

// =========================================================================
// PUBLIC HELPER FUNCTIONS
// =========================================================================

/// Returns the Atomic Number (Z).
///
/// **Source:** IUPAC Periodic Table.
pub fn get_atomic_number(element: &str) -> i32 {
    get_atom_data(element).atomic_number
}

/// Returns the Cromer-Mann Coefficients [a1, b1, a2, b2, a3, b3, a4, b4, c].
/// Used for calculating Atomic Scattering Factors for X-rays.
///
/// **Source:** International Tables for Crystallography, Vol. C, Table 6.1.1.4 (1992).
pub fn get_cromer_mann_coeffs(element: &str) -> [f64; 9] {
    get_atom_data(element).cromer_mann
}

/// Returns the Shannon Ionic Radius (Angstroms) for the most common oxidation state (CN=6).
///
/// **Source:** Shannon, R.D. "Revised effective ionic radii and systematic studies of interatomic distances in halides and chalcogenides."
/// *Acta Crystallographica Section A*, 32, 751-767 (1976).
/// DOI: [10.1107/S056773947600155X](https://doi.org/10.1107/S056773947600155X)
//  RENAMED BACK TO get_atom_ionic_radius to preserve backward compatibility
pub fn get_atom_ionic_radius(element: &str) -> f64 {
    get_atom_data(element).ionic_radius
}

/// Returns the Van der Waals Radius (Angstroms).
///
/// **Source:** Alvarez, S. "A cartography of the van der Waals territory."
/// *Dalton Transactions*, 42, 8617-8636 (2013).
/// DOI: [10.1039/C3DT50599E](https://doi.org/10.1039/C3DT50599E)
//  RENAMED BACK TO get_atom_vdw to preserve backward compatibility
pub fn get_atom_vdw(element: &str) -> f64 {
    get_atom_data(element).vdw_radius
}

/// Returns the Covalent Radius (Angstroms).
///
/// **Source:** Cordero, B. et al. "Covalent radii revisited."
/// *Dalton Transactions*, 2832-2838 (2008).
/// DOI: [10.1039/B801115J](https://doi.org/10.1039/B801115J)
pub fn get_covalent_radius(element: &str) -> f64 {
    get_atom_data(element).covalent_radius
}

/// Returns the Pauling Electronegativity.
///
/// **Source:** Allred, A. L. "Electronegativity values from thermochemical data."
/// *Journal of Inorganic and Nuclear Chemistry*, 17(3-4), 215-221 (1961).
/// DOI: [10.1016/0022-1902(61)80142-5](https://doi.org/10.1016/0022-1902(61)80142-5)
pub fn get_electronegativity(element: &str) -> f64 {
    get_atom_data(element).electronegativity
}

/// Returns the CPK/Material Design Color tuple (R, G, B).
///
/// **Source:** Material Design Colors adapted for standard CPK distinctions.
pub fn get_cpk_color(element: &str) -> (f64, f64, f64) {
    get_atom_data(element).color_rgb
}

/// Legacy wrapper for existing code compatibility.
/// Returns (Covalent Radius, RGB Color).
pub fn get_atom_properties(element: &str) -> (f64, (f64, f64, f64)) {
    let d = get_atom_data(element);
    (d.covalent_radius, d.color_rgb)
}

/// Legacy wrapper for existing code compatibility.
/// Returns just the Covalent Radius.
pub fn get_atom_cov(element: &str) -> f64 {
    get_atom_data(element).covalent_radius
}
