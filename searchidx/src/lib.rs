//! Platform-independent full-text index for note.md vaults.
//!
//! The index is a **pure function of the vault's files** (plus the tokenizer
//! version) stored outside the vault, in the machine's local app cache. It is
//! disposable: any inconsistency is resolved by deleting and rebuilding, never
//! by repair logic. Nothing here ever writes into the vault.
//!
//! Everything that decides *what gets indexed and what ranks first* lives in
//! this crate so that the Tauri command layer, the `notemd search` CLI and the
//! file watcher are three thin adapters over one algorithm. That is the whole
//! reason the crate exists — see docs/2026-08-10-vault-search-index-design.md §2.

pub mod block;
pub mod frontmatter;
pub mod norm;
pub mod prose;
pub mod tokenize;
