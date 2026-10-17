//! Analysis kernels. Mesh coordinates are SI metres, not CAD millimetres.
//! No mesher, persisted study, native UI or automatic material suitability claim.
mod element;
mod steady;
mod surface;
pub use steady::{solve_steady, solve_steady_with_surfaces};
#[derive(Debug, Clone, PartialEq)]
pub struct ThermalTet {
    pub nodes: [usize; 4],
    pub material: usize,
    /// Constant volumetric heat source, W/m^3; negative means cooling.
    pub source_w_m3: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ThermalMesh {
    pub points_m: Vec<[f64; 3]>,
    pub elements: Vec<ThermalTet>,
}
#[derive(Debug, Clone, Copy)]
pub struct SolveOptions {
    pub max_nodes: usize,
    pub max_elements: usize,
    pub max_iterations: usize,
    pub relative_tolerance: f64,
    pub absolute_tolerance_w: f64,
}
impl Default for SolveOptions {
    fn default() -> Self {
        Self {
            max_nodes: 4096,
            max_elements: 16384,
            max_iterations: 10000,
            relative_tolerance: 1e-10,
            absolute_tolerance_w: 1e-10,
        }
    }
}
#[derive(Debug, Clone)]
pub struct ThermalResult {
    pub temperature_k: Vec<f64>,
    /// Heat supplied by prescribed-temperature constraints, W. Zero on free
    /// nodes; positive means heat entering the domain. Sum balances all loads.
    pub reaction_w: Vec<f64>,
    /// Constant element flux in input element order, W/m^2.
    pub element_flux_w_m2: Vec<[f64; 3]>,
    /// Net heat entering through each supplied surface, W, in input order.
    pub surface_heat_w: Vec<f64>,
    pub iterations: usize,
    pub free_residual_l2_w: f64,
    pub balance_error_w: f64,
}

/// Constant convection against an ambient temperature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThermalConvection {
    pub coefficient_w_m2_k: f64,
    pub ambient_k: f64,
}

/// One exterior mesh triangle. Order does not define flux direction: positive
/// heat_flux_in_w_m2 always enters the domain. Flux and convection may coexist.
/// Use at most one record per face; prescribed nodal temperatures may overlap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThermalSurface {
    pub nodes: [usize; 3],
    pub heat_flux_in_w_m2: f64,
    pub convection: Option<ThermalConvection>,
}
