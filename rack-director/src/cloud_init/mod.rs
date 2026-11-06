//! Cloud-config delivered to provisioned machines through cloud-init.
//!
//! Operators write cloud-config in two layers — a global one (`settings`) and one per role —
//! which are validated on save and merged when a device fetches its NoCloud seed on first
//! boot. OSM install templates opt in (`cloud_init = true`) and run [`install_script`] so the
//! installed system knows where to fetch the seed and reports back when cloud-init is done.

mod install;
mod merge;
mod parse;
mod seed;

pub use install::{install_script, seed_path, seed_url};
pub use parse::validate;
pub use seed::{CloudInitSeed, SeedDevice, build_seed};
