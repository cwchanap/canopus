//! Daemon library for the Canopus project

#![allow(unused_crate_dependencies)]

pub mod simple_error;
pub mod bootstrap;

#[cfg(test)]
mod simple_error_tests;

use schema::{DaemonConfig, Message, Response};
pub use simple_error::{DaemonError, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, warn};

/// The main daemon server
#[derive(Debug)]
pub struct Daemon {
    config: DaemonConfig,
    start_time: Instant,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl Daemon {
    /// Create a new daemon instance
    pub fn new(config: DaemonConfig) -> Self {
        Self {
            config,
            start_time: Instant::now(),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the daemon server
    pub async fn start(&self) -> Result<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| DaemonError::ServerError(format!("Failed to bind to {}: {}", addr, e)))?;

        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        info!("Daemon started on {}", addr);

        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New connection from {}", addr);
                    let daemon = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = daemon.handle_connection(stream).await {
                            error!("Error handling connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Handle incoming connection
    async fn handle_connection(&self, mut stream: TcpStream) -> Result<()> {
        let mut buffer = [0; 1024];

        loop {
            let n = stream.read(&mut buffer).await?;
            if n == 0 {
                break;
            }

            let request: Message = serde_json::from_slice(&buffer[..n])?;
            let response = self.process_message(request).await;
            let response_data = serde_json::to_vec(&response)?;

            stream.write_all(&response_data).await?;
        }

        Ok(())
    }

    /// Process incoming messages
    async fn process_message(&self, message: Message) -> Response {
        match message {
            Message::Status => {
                let uptime_seconds = self.start_time.elapsed().as_secs();
                let running = self.running.load(std::sync::atomic::Ordering::SeqCst);
                Response::Status {
                    running,
                    uptime_seconds,
                    version: Some("0.1.0".to_string()),
                }
            }
            Message::Start => {
                if self.running.load(std::sync::atomic::Ordering::SeqCst) {
                    Response::Error {
                        message: "Daemon is already running".to_string(),
                        code: Some("DAEMON_ALREADY_RUNNING".to_string()),
                    }
                } else {
                    self.running
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    Response::Ok {
                        message: "Daemon started".to_string(),
                    }
                }
            }
            Message::Stop => {
                self.running
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                Response::Ok {
                    message: "Daemon stopping".to_string(),
                }
            }
            Message::Restart => {
                warn!("Restart requested - this is a simplified implementation");
                Response::Ok {
                    message: "Restart acknowledged".to_string(),
                }
            }
            Message::Custom { cmd } => {
                info!("Custom command received: {}", cmd);
                Response::Ok {
                    message: format!("Processed: {}", cmd),
                }
            }
        }
    }

    /// Stop the daemon
    pub fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Clone for Daemon {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            start_time: self.start_time,
            running: Arc::clone(&self.running),
        }
    }
}
