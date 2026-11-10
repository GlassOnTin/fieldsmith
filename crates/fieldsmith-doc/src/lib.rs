//! Fieldsmith document model, `.fsm` text format and material library.

pub mod command;
pub mod fixings;
pub mod fsm;
pub mod loaddist;
pub mod materials;
pub mod model;
pub mod param;
pub mod sketch;
pub mod spatial;

pub use materials::{Material, MaterialRole};
pub use model::*;
