// src/structure.rs

#[derive(Clone, Debug)]
pub struct Atom {
    pub element: String,
    pub position: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct Structure {
    pub lattice: [[f64; 3]; 3],
    pub atoms: Vec<Atom>,
}
