//! Core data model and file format for the Kernest font editor.
//!
//! This crate has no UI or platform dependencies. It owns:
//! - the in-memory font model ([`Font`], [`Master`], [`Glyph`], [`Layer`], …),
//! - outline geometry helpers (bounds, sidebearings, SVG path output),
//! - basic glyph data (names and categories for Unicode code points),
//! - the `.kernest` package format: a folder with one JSON file per glyph,
//!   so fonts diff and merge cleanly in Git.

pub mod error;
pub mod geometry;
pub mod glyphdata;
pub mod model;
pub mod package;
pub mod sample;

pub use error::{Error, Result};
pub use geometry::Rect;
pub use model::*;
