//! Local server diagnostics. Never print session credentials or request bodies.
use orbit_research_core::Error;

pub(crate) fn listening(origin: &str) -> String {
    format!("Orbit Research: {origin}")
}

pub(crate) fn request_failed(error: &Error) -> String {
    format!("Orbit Research request failed: {error}")
}
