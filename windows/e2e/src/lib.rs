//! The portable half of the e2e harness, so `cargo test` checks its verdict logic on any OS before a runner does.
pub mod checks;
pub mod judge;
pub mod pixels;
