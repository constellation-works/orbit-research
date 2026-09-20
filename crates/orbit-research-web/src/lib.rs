//! Loopback dashboard server composition.
mod api;
mod log_format;
mod parse;

use orbit_research_core::{Application, Error, Research as Corpus, Result};
use std::io::Read;
use tiny_http::Server;

pub fn serve(corpus: Corpus, port: u16) -> Result<()> {
    let root = corpus.root().to_owned();
    serve_application(Application::local(&root)?, port)
}

pub fn serve_application(application: Application, port: u16) -> Result<()> {
    let server = Server::http(("127.0.0.1", port)).map_err(|e| Error::Invalid(e.to_string()))?;
    let address = server.server_addr().to_string();
    let origin = format!("http://{address}");
    let mut random = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let token = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    eprintln!("{}", log_format::listening(&origin));
    for request in server.incoming_requests() {
        if let Err(error) = api::handle_request(request, &application, &address, &origin, &token) {
            eprintln!("{}", log_format::request_failed(&error));
        }
    }
    Ok(())
}
