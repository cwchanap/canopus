#![allow(unused_crate_dependencies)]
//! Simple HTTP (HTTP/1.1) toy service for E2E tests
//!
//! Behavior:
//! - Reads `PORT` env var or first CLI arg (default 8082)
//! - Prints "ready" to stdout immediately
//! - Serves a minimal HTTP/1.1 200 OK with body "ok" for any path
//! - For `/health` responds with body "healthy"
//! - Single-threaded accept loop, per-connection handler thread

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn handle_client(mut stream: TcpStream) {
    // Read a small buffer to get the request line; we don't implement full HTTP parsing
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf);

    // Naive path detection from the first line
    let req = String::from_utf8_lossy(&buf);
    let first_line = req.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/");

    let (status_line, body) = if path == "/health" {
        ("HTTP/1.1 200 OK\r\n", "healthy")
    } else {
        ("HTTP/1.1 200 OK\r\n", "ok")
    };

    let headers = format!(
        "{}Content-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status_line,
        body.len()
    );

    let _ = stream.write_all(headers.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .or_else(|| std::env::args().nth(1).and_then(|s| s.parse().ok()))
        .unwrap_or(8082);

    let listener = TcpListener::bind(("0.0.0.0", port))?;

    println!("ready");
    let _ = std::io::stdout().flush();

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                thread::spawn(|| handle_client(s));
            }
            Err(_e) => break,
        }
    }
    Ok(())
}
