//! Shared test helpers. No external network: the HTTP server below is a plain
//! `TcpListener` on 127.0.0.1 with an ephemeral port.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use w3b::app::{App, Viewport};
use w3b::theme::Theme;
use w3b::FakeFetcher;

/// A scripted HTTP server. `routes` maps a request path to a response.
pub struct TestServer {
    pub base: String,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    pub requests: Arc<std::sync::Mutex<Vec<String>>>,
}

/// status, content type, body
#[derive(Clone)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub extra_headers: Vec<String>,
}

impl Response {
    pub fn html(status: u16, body: &str) -> Response {
        Response {
            status,
            content_type: "text/html; charset=utf-8".into(),
            body: body.as_bytes().to_vec(),
            extra_headers: Vec::new(),
        }
    }
    pub fn bytes(status: u16, content_type: &str, body: &[u8]) -> Response {
        Response { status, content_type: content_type.into(), body: body.to_vec(), extra_headers: Vec::new() }
    }
    pub fn header(mut self, key: &str, value: &str) -> Response {
        self.extra_headers.push(format!("{key}: {value}"));
        self
    }
}

impl TestServer {
    pub fn start(routes: Vec<(&str, Response)>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        // Non-blocking so Drop can join the accept thread instead of hanging.
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let routes: Vec<(String, Response)> = routes.into_iter().map(|(p, r)| (p.to_string(), r)).collect();

        let stop2 = Arc::clone(&stop);
        let requests2 = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                let stream = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => return,
                };
                let routes = routes.clone();
                let requests = Arc::clone(&requests2);
                thread::spawn(move || {
                    let _ = serve_one(stream, &routes, &requests);
                });
            }
        });

        TestServer { base: format!("http://127.0.0.1:{port}"), stop, handle: Some(handle), requests }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub fn request_paths(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn serve_one(
    mut stream: TcpStream,
    routes: &[(String, Response)],
    requests: &Arc<std::sync::Mutex<Vec<String>>>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_string();
    requests.lock().unwrap().push(path.clone());
    // Drain the headers so the client does not see a reset mid-request.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
    }

    let query = path.split('?').next().unwrap_or("/").to_string();
    let response = routes
        .iter()
        .find(|(p, _)| *p == path || *p == query)
        .map(|(_, r)| r.clone())
        .unwrap_or_else(|| Response::html(404, "<h1>no such route</h1>"));

    let reason = w3b::status::Status::http(response.status).text();
    let mut out = format!("HTTP/1.1 {} {}\r\n", response.status, reason);
    out += &format!("Content-Type: {}\r\n", response.content_type);
    out += &format!("Content-Length: {}\r\n", response.body.len());
    for h in &response.extra_headers {
        out += &format!("{h}\r\n");
    }
    out += "Connection: close\r\n\r\n";
    stream.write_all(out.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

/// Build an App wired to a fake fetcher, no threads, no terminal.
/// App with the start page already loaded (synchronous fetch, no threads).
pub fn fake_app(fetcher: FakeFetcher, start_url: &str, width: usize, height: usize) -> App {
    let config = w3b::config::built_in_defaults();
    let theme = Theme::build(&config).expect("theme");
    let mut app = App::empty(config, theme, Arc::new(fetcher), start_url, Viewport { width, height });
    app.open(start_url);
    app
}

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

pub fn char_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

pub fn press(app: &mut App, code: KeyCode) -> w3b::Action {
    app.handle_key(key(code))
}

pub fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(char_key(c));
    }
}

pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("fixtures/{name}")).expect("fixture present")
}
