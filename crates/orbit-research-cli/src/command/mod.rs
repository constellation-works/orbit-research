//! Command declarations and focused execution helpers.
//!
//! Parsing types live in [`crate::parse`]; this module owns the research and
//! workspace command seams so `main` remains composition and process policy.
pub mod application;
pub mod research;
pub mod resource;
pub mod workspace;
