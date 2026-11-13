//! Core data model and file format for the Kernest font editor.
//!
//! This crate has no UI or platform dependencies. It owns:
//! - the in-memory font model ([`Font`], [`Master`], [`Glyph`], [`Layer`], …),
//! - outline geometry helpers (bounds, sidebearings, SVG path output),
//! - basic glyph data (names and categories for Unicode code points),
//! - the `.kernest` package format: a folder with one JSON file per glyph,
//!   so fonts diff and merge cleanly in Git.

pub mod designspace;
pub mod edit;
pub mod error;
pub mod geometry;
pub mod glyphdata;
pub mod glyphset;
pub mod interpolate;
pub mod kerning;
pub mod model;
pub mod package;
pub mod pathops;
pub mod sample;
pub mod structure;
pub mod text;

pub use error::{Error, Result};
pub use geometry::Rect;
pub use model::*;
