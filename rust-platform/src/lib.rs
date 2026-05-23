#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::derivable_impls,
    clippy::doc_lazy_continuation,
    clippy::items_after_test_module,
    clippy::large_enum_variant,
    clippy::manual_map,
    clippy::manual_strip,
    clippy::needless_borrow,
    clippy::too_many_arguments,
    clippy::useless_vec
)]

pub mod agent;
pub mod cli;
pub mod config;
pub mod error;
pub mod logging;
pub mod models;
pub mod orchestrator;
pub mod platform;
pub mod prompt;
pub mod server;
pub mod tools;
pub mod tracker;
pub mod workspace;
