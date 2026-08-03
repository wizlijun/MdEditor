//! The roam-import plugin's library half. Everything except the stdio server
//! entry point (`main.rs`) and the plugin trait impl (`plugin.rs`) lives here,
//! because `tests/golden.rs` — the shared format-drift fixture asserted from
//! both Rust and TypeScript — is an integration test, and an integration test
//! can only reach a library crate.
pub mod convert;
pub mod dates;
pub mod discover;
pub mod ledger;
pub mod merge;
pub mod outline;
mod procutil;
pub mod roam_cli;
pub mod roam_page;
pub mod route;
pub mod sync;
pub mod syntax;
