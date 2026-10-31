//! The local hand-off link. Bookmarks open through `http://127.0.0.1:<port>/zephyr/open?u=…`
//! so the browser extension can see them and focus a tab already on that page. Without the
//! extension the link simply redirects.

use std::sync::atomic::{AtomicU16, Ordering};

use crate::util::LogErr;

static PORT: AtomicU16 = AtomicU16::new(0);

/// The listener's port; 0 until it is up (or if it couldn't start).
pub fn port() -> u16 {
    PORT.load(Ordering::SeqCst)
}

/// Binds the listener on a random local port and serves it on its own thread.
pub fn start() {
    let server = match tiny_http::Server::http("127.0.0.1:0") {
        Ok(server) => server,
        Err(err) => {
            log::error!("couldn't start the hand-off listener: {err}");
            return;
        }
    };
    if let Some(addr) = server.server_addr().to_ip() {
        PORT.store(addr.port(), Ordering::SeqCst);
    }
    std::thread::Builder::new()
        .name("handoff".into())
        .spawn(move || {
            for request in server.incoming_requests() {
                handle(request);
            }
        })
        .log_err("couldn't start the handoff server");
}

fn handle(request: tiny_http::Request) {
    let target = request
        .url()
        .strip_prefix("/zephyr/open?")
        .and_then(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "u")
                .map(|(_, value)| value.into_owned())
        })
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"));
    let Some(target) = target else {
        let _ = request.respond(tiny_http::Response::empty(404));
        return;
    };
    let response = tiny_http::Response::empty(302);
    let _ = match tiny_http::Header::from_bytes("Location", target.as_bytes()) {
        Ok(location) => request.respond(response.with_header(location)),
        Err(()) => request.respond(tiny_http::Response::empty(400)),
    };
}
