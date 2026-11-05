//! The portable half of the e2e harness: the check list, the report, and every judgement over the facts the Windows
//! half gathers. `cargo test` runs it on any OS, so the logic that turns facts into PASS or FAIL never runs for the
//! first time on a billed runner.
pub mod checks;
pub mod judge;
pub mod pixels;
