//! The one-off listener on 127.0.0.1 that a browser sign-in (OAuth with PKCE) comes back to,
//! shared by Sign in with ChatGPT and sync's Google sign-in.

use std::time::{Duration, Instant};

/// A listener on the first free port of `ports`, and the redirect address it answers at.
pub fn listen(ports: &[u16]) -> Result<(tiny_http::Server, String), String> {
    let server = ports
        .iter()
        .find_map(|port| tiny_http::Server::http(("127.0.0.1", *port)).ok())
        .ok_or("Couldn't open a local port for sign-in")?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .ok_or("Couldn't open a local port for sign-in")?;
    Ok((server, format!("http://127.0.0.1:{port}/auth/callback")))
}

/// Waits up to five minutes for the browser to come back, stopping once `cancelled` says so.
/// `read` turns the callback's query parameters into the result; the tab then shows `done`,
/// or on an error, a note to go back and try again.
pub fn wait<T>(
    server: &tiny_http::Server,
    cancelled: impl Fn() -> bool,
    done: &str,
    read: impl Fn(&dyn Fn(&str) -> Option<String>) -> Result<T, String>,
) -> Result<T, String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        if cancelled() {
            return Err("Sign-in was cancelled".into());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("Sign-in timed out. Try again.".into());
        }
        let Ok(Some(request)) = server.recv_timeout(left.min(Duration::from_secs(1))) else {
            continue;
        };
        if !request.url().starts_with("/auth/callback") {
            let _ = request.respond(tiny_http::Response::empty(404));
            continue;
        }
        let url = url::Url::parse(&format!("http://127.0.0.1{}", request.url())).ok();
        let param = |name: &str| {
            url.as_ref().and_then(|url| {
                url.query_pairs()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.into_owned())
            })
        };
        let result = read(&param);
        let page = if result.is_ok() {
            done
        } else {
            "Zephyr couldn't finish signing in. Go back to Zephyr and try again."
        };
        let header = tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8")
            .expect("static header");
        let html = format!(
            "<!doctype html><meta charset=utf-8><title>Zephyr</title><body style=\"font:16px \
             -apple-system,system-ui;background:#16131f;color:#ececee;display:grid;\
             place-items:center;height:100vh;margin:0\"><p>{page}</p>"
        );
        let _ = request.respond(tiny_http::Response::from_string(html).with_header(header));
        return result;
    }
}
