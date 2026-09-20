//! CLI output exports.
mod render;

pub(crate) use render::{Invalid, OutputMode, invalid, render_with_terminal};

#[cfg(test)]
mod tests;
