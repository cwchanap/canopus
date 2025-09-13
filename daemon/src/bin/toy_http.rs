#![cfg_attr(test, allow(unused_crate_dependencies))]
//! Tiny TCP toy service for E2E tests (no HTTP)
//!
//! Behavior:
//! - Reads `PORT` env var or first CLI arg (default 8081)
//! - Prints "ready" to stdout immediately
//! - Accepts TCP connections and closes them (no protocol)

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;

fn handle_client(mut stream: TcpStream) {
    // No protocol: accept and immediately close after a tiny write
    let _ = stream.write_all(b"ready\n");
    let _ = stream.flush();
}

fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .or_else(|| std::env::args().nth(1).and_then(|s| s.parse().ok()))
        .unwrap_or(8081);
    let listener = TcpListener::bind(("0.0.0.0", port))?;

    println!("ready");
    std::io::stdout().flush().ok();

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
