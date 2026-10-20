//! The link to the browser extension: a local WebSocket the extension keeps open, so
//! the bar can ask it to switch to a tab that's already on a page instead of opening the page
//! again (no new tab appears at all). Only browser extensions may connect.

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

/// Fixed so the extension knows where to connect.
pub const PORT: u16 = 47391;

struct Client {
    id: u64,
    outbox: Sender<String>,
}

struct Waiting {
    request: u64,
    reply: Sender<bool>,
}

fn clients() -> &'static Mutex<Vec<Client>> {
    static CLIENTS: OnceLock<Mutex<Vec<Client>>> = OnceLock::new();
    CLIENTS.get_or_init(Default::default)
}

fn waiting() -> &'static Mutex<Vec<Waiting>> {
    static WAITING: OnceLock<Mutex<Vec<Waiting>>> = OnceLock::new();
    WAITING.get_or_init(Default::default)
}

static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn start() {
    let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("couldn't listen for the browser extension on {PORT}: {err}");
            return;
        }
    };
    std::thread::Builder::new()
        .name("browser-link".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || serve(stream));
            }
        })
        .ok();
}

/// Whether a connection comes from a browser extension; web pages can't use this socket.
fn from_extension(origin: Option<&str>) -> bool {
    origin.is_some_and(|origin| {
        origin.starts_with("chrome-extension://") || origin.starts_with("extension://")
    })
}

// tungstenite's handshake callback returns its own (large) error type.
#[allow(clippy::result_large_err)]
fn serve(stream: TcpStream) {
    let socket = tungstenite::accept_hdr(
        stream,
        |request: &tungstenite::handshake::server::Request, response| {
            let origin = request
                .headers()
                .get("Origin")
                .and_then(|value| value.to_str().ok());
            if from_extension(origin) {
                Ok(response)
            } else {
                Err(tungstenite::http::Response::builder()
                    .status(403)
                    .body(None)
                    .expect("static response"))
            }
        },
    );
    let Ok(mut socket) = socket else {
        return;
    };
    let _ = socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(50)));
    let (outbox, inbox) = mpsc::channel();
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut clients) = clients().lock() {
        clients.push(Client { id, outbox });
    }
    log::info!("browser extension connected");
    pump(&mut socket, &inbox);
    if let Ok(mut clients) = clients().lock() {
        clients.retain(|client| client.id != id);
    }
    log::info!("browser extension disconnected");
}

/// Sends what's queued, reads what arrives, and pings often enough that the browser keeps
/// the extension's background worker (and so the socket) alive.
fn pump(socket: &mut WebSocket<TcpStream>, inbox: &Receiver<String>) {
    let mut pinged = Instant::now();
    loop {
        while let Ok(text) = inbox.try_recv() {
            if socket.send(Message::text(text)).is_err() {
                return;
            }
        }
        if pinged.elapsed() > Duration::from_secs(20) {
            pinged = Instant::now();
            if socket
                .send(Message::text(json!({ "type": "ping" }).to_string()))
                .is_err()
            {
                return;
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => received(text.as_str()),
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(err))
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

fn received(text: &str) {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if message["type"] == "focused"
        && let Some(request) = message["id"].as_u64()
        && let Ok(waiting) = waiting().lock()
        && let Some(wait) = waiting.iter().find(|wait| wait.request == request)
    {
        let _ = wait.reply.send(message["found"].as_bool().unwrap_or(false));
    }
}

/// Asks the extension to switch to a tab already on `url`. `Some(true)` when it did,
/// `Some(false)` when no tab is on that page, `None` when no extension is connected (or it
/// didn't answer in time).
pub fn focus(url: &str) -> Option<bool> {
    ask(json!({ "type": "focus", "url": url }))
}

/// Asks the extension to switch to the tab playing media: the audible one, or the one whose
/// title has the track's title when it's paused. Answers as [`focus`] does.
pub fn focus_playing(title: &str) -> Option<bool> {
    ask(json!({ "type": "focusPlaying", "title": title }))
}

/// Sends a request to every connected browser and waits briefly for one to say it found
/// the tab.
fn ask(mut message: Value) -> Option<bool> {
    let outboxes: Vec<Sender<String>> = clients()
        .lock()
        .ok()?
        .iter()
        .map(|client| client.outbox.clone())
        .collect();
    if outboxes.is_empty() {
        return None;
    }
    let request = NEXT.fetch_add(1, Ordering::SeqCst);
    let (reply, answer) = mpsc::channel();
    waiting().lock().ok()?.push(Waiting { request, reply });
    message["id"] = json!(request);
    let message = message.to_string();
    for outbox in &outboxes {
        let _ = outbox.send(message.clone());
    }
    // Every connected browser answers; any one that has the tab wins.
    let deadline = Instant::now() + Duration::from_millis(600);
    let mut answered = 0;
    let mut found = None;
    while answered < outboxes.len() {
        let left = deadline.saturating_duration_since(Instant::now());
        match answer.recv_timeout(left) {
            Ok(true) => {
                found = Some(true);
                break;
            }
            Ok(false) => {
                answered += 1;
                found = Some(false);
            }
            Err(_) => break,
        }
    }
    if let Ok(mut waiting) = waiting().lock() {
        waiting.retain(|wait| wait.request != request);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_extensions_may_connect() {
        assert!(from_extension(Some("chrome-extension://abcdefghijklmnop")));
        assert!(!from_extension(Some("https://evil.example")));
        assert!(!from_extension(Some("null")));
        assert!(!from_extension(None));
    }
}
