//! Research use cases shared by CLI, MCP and Web.
pub mod api;
pub mod operations;
pub mod receipt;
pub mod work;

mod operation;
mod request;
pub use operation::{Operation, ToolDefinition};

#[cfg(test)]
mod tests;
