//! Quadratic (P2) tetrahedral linear elasticity.
//!
//! * [`P2Mesh`] adds edge-midpoint nodes to a linear tetrahedral mesh, numbered vertices first.
//!   Local node order follows `fenics_element::P2Tetrahedron`.
//! * [`assemble`] builds the stiffness matrix in 3×3 block-CSR form using fenics-rs reference
//!   basis gradients, its degree-2 Keast rule (exact for P2 stiffness on affine elements) and
//!   its affine Jacobian map.
//! * [`solve`] runs preconditioned conjugate gradients with a two-level preconditioner:
//!   block symmetric Gauss–Seidel smoothing on the P2 system and an exact (sparse Cholesky,
//!   faer) solve on the embedded P1 system.
//! * [`stress`] recovers nodal stresses by volume-weighted averaging.

pub mod assemble;
pub mod bsr;
pub mod modal;
pub mod p2;
pub mod pcg;
pub mod stress;
pub mod thermal;

pub use assemble::assemble;
pub use bsr::Bsr;
pub use modal::{solve_modes, ModalResult, ModeOptions};
pub use p2::P2Mesh;
pub use pcg::{solve, SolveReport, SolverOptions};
pub use stress::{
    element_stress, element_stress_thermal, nodal_stress, nodal_stress_thermal, von_mises, Stress,
};
pub use thermal::{
    assemble_laplace, element_laplace, element_robin, element_thermal_integrals, solve_conduction,
    thermal_load, Csr, RobinBc, ThermalRefData,
};

pub type V3 = [f64; 3];

/// Isotropic material, one Young's modulus per element (graded fill scales it), MPa.
#[derive(Clone, Debug)]
pub struct Material {
    pub young: Vec<f64>,
    pub poisson: f64,
}

/// A static linear-elastic problem on a P2 mesh. Units: mm, N, MPa.
#[derive(Clone, Debug)]
pub struct Problem {
    pub mesh: P2Mesh,
    pub material: Material,
    /// Per node: bit 0/1/2 set when the x/y/z displacement is fixed at zero.
    pub fixed: Vec<u8>,
    /// Nodal forces, 3 per node (N).
    pub force: Vec<f64>,
}
