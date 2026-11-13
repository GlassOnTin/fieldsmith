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
//! * [`bounds::compliance`] reads the raw-force compliance fᵀu: by Galerkin orthogonality
//!   this is a(u,u) ≥ 0 and a certified lower bound C_h ≤ C_true on the exact compliance.
//! * [`dual::upper_compliance`] recovers an equilibrated stress field σ* whose complementary
//!   energy carries the upper end against the same load: exact when the exact field is
//!   representable, and certified up to the P2 interpolation defect otherwise.

pub mod antenna;
pub mod assemble;
pub mod bounds;
pub mod bsr;
pub mod candle_flame;
pub mod contact;
pub mod coupled_system;
pub mod dual;
pub mod fixings;
pub mod float_bath;
pub mod modal;
pub mod ortho;
pub mod p2;
pub mod pcg;
pub mod sph_surface_tension;
pub mod stress;
pub mod thermal;

pub use assemble::assemble;
pub use bounds::compliance;
pub use bsr::Bsr;
pub use contact::{
    add_converged_contact_tangent, solve_unilateral_contact, ContactFacet, ContactPatch,
    ContactReport, ContactSolveOutput,
};
pub use dual::{upper_compliance, UpperBound};
pub use fenics_assembly::elasticity::ElasticMaterial;
pub use modal::{modal_mass_quadratic_form, solve_modes, ModalResult, ModeOptions};
pub use p2::P2Mesh;
pub use pcg::{
    benchmark_large_gpu_fea, discover_local_gpus, solve, solve_gpu, solve_multi_rhs,
    LargeGpuFeaBenchmark, SolveReport, SolverOptions,
};
pub use stress::{
    element_strain_energy_2u, element_stress, element_stress_cubic, element_stress_mat,
    element_stress_thermal, layer_peel_quadratic, layer_peel_utilization, layer_tractions,
    min_compressive, nodal_stress, nodal_stress_mat, nodal_stress_thermal,
    nodal_stress_thermal_multi, principal_stresses, von_mises, Stress,
};
pub use thermal::{
    assemble_conduction_system_spatial, assemble_laplace, assemble_laplace_k,
    assemble_thermal_mass, element_flux, element_flux_spatial, element_laplace,
    element_pressure_spatial, element_robin, element_robin_spatial, element_thermal_integrals,
    solve_conduction, solve_conduction_multi_rhs, solve_conduction_spatial,
    solve_transient_conduction, solve_transient_conduction_spatial, thermal_load,
    thermal_load_multi, thermal_load_transpose_multi, Csr, HeatFluxBc, RobinBc, SpatialHeatFluxBc,
    SpatialRobinBc, ThermalRefData, TransientConductionFrame,
};

pub type V3 = [f64; 3];

/// Static-elastic material: one Young's modulus per element (graded fill scales it),
/// MPa, with optional per-element CUBIC stiffness triples (the anisotropic lattice law,
/// `ortho`): elements whose slot carries a triple assemble and recover through the cubic
/// kernel, the rest through the isotropic one (unchanged). `None` (or a missing slot) =
/// the isotropic path for that element.
#[derive(Clone, Debug)]
pub struct Material {
    pub young: Vec<f64>,
    pub poisson: f64,
    pub cubic: Option<Vec<Option<crate::ortho::CubicMaterial>>>,
}

impl Material {
    /// The isotropic-only material (no cubic elements) — every pre-existing caller.
    pub fn iso(young: Vec<f64>, poisson: f64) -> Material {
        Material {
            young,
            poisson,
            cubic: None,
        }
    }
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
