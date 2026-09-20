//! HTTP boundary and route exports.
mod request;
mod router;

pub(crate) use router::handle_request;

#[cfg(test)]
mod tests;
