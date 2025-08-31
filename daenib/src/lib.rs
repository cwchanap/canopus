use core::{Config, Message, Response, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, error, warn};

/// The main daemon server
pub struct Daemon {
    config: Config,
    start_time: Instant,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl Daemon {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            start_time: Instant::now(),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the daemon server
    pub async fn start(&self) -> Result<()> {
        let addr = format!("{}:{}", self.config.daemon_host, self.config.daemon_port);
        let listener = TcpListener::bind(&addr).await?;
        
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);
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
                let uptime = self.start_time.elapsed().as_secs();
                let running = self.running.load(std::sync::atomic::Ordering::SeqCst);
                Response::Status { running, uptime }
            }
            Message::Start => {
                if self.running.load(std::sync::atomic::Ordering::SeqCst) {
                    Response::Error("Daemon is already running".to_string())
                } else {
                    self.running.store(true, std::sync::atomic::Ordering::SeqCst);
                    Response::Ok("Daemon started".to_string())
                }
            }
            Message::Stop => {
                self.running.store(false, std::sync::atomic::Ordering::SeqCst);
                Response::Ok("Daemon stopping".to_string())
            }
            Message::Restart => {
                warn!("Restart requested - this is a simplified implementation");
                Response::Ok("Restart acknowledged".to_string())
            }
            Message::Custom(cmd) => {
                info!("Custom command received: {}", cmd);
                Response::Ok(format!("Processed: {}", cmd))
            }
        }
    }

    /// Stop the daemon
    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
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
