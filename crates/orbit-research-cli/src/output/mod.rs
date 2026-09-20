//! Output policy and rendering exports.
mod render;
mod sink;
mod table;

pub(crate) use render::{Invalid, render, render_error};
pub(crate) use sink::{OutputMode, OutputSink, error_format};

#[cfg(test)]
mod tests;
