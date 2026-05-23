#![allow(
    unused_imports,
    unused_variables,
    dead_code,
    clippy::bind_instead_of_map,
    clippy::derivable_impls,
    clippy::manual_range_contains,
    clippy::needless_borrows_for_generic_args,
    clippy::ptr_arg,
    clippy::duplicated_attributes,
    clippy::approx_constant,
    clippy::bool_assert_comparison,
    clippy::len_zero,
    clippy::let_and_return
)]

//! Unit test modules for Symphony platform adapter.

mod codex_protocol_tests;
mod config_tests;
mod orchestrator_tests;
mod prompt_tests;
mod reconciler_tests;
mod retry_tests;
mod scheduler_tests;
mod token_tests;
mod workspace_tests;
