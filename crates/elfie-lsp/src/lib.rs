//! Compiled from `def/lsp/main.lfy`: the Elfie language server.
//!
//! [`serve`] speaks the Language Server Protocol over standard input and output for one
//! editor until it shuts the server down. Every feature is one query of
//! [`elfie_core::query`] over the [`Session`]'s workspace; the server converts document
//! URIs to workspace paths and protocol positions to token positions at its edge, and
//! nothing else. The protocol plumbing is the `tower-lsp` library, the external
//! declaration `Protocol` of the definition.
// @lfy def/lsp/main.lfy:Protocol

mod convert;
mod data;
mod server;

#[cfg(test)]
mod serve_tests;

pub use data::*;
pub use server::{Backend, serve};

/// The project layout file, whose change reloads the whole workspace.
const MANIFEST: &str = "elfie.json";
/// The message of a rename request on nothing.
const NOTHING_DECLARED: &str = "nothing is declared here";
