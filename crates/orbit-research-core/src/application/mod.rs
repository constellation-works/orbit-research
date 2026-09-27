//! Research use cases shared by CLI, MCP and Web.
pub mod acceptance;
pub mod api;
pub mod operations;
pub mod work;

mod operation;
mod request;
pub use operation::{Operation, ToolDefinition};

#[cfg(test)]
mod tests;
