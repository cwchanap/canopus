//! TCP connection health probing

use async_trait::async_trait;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::debug;

use super::{HealthError, Probe};

/// TCP health probe that tests connection establishment
///
/// This probe attempts to establish a TCP connection to the specified
/// host and port. The connection is immediately closed after establishment.
///
/// # Example
///
/// ```rust
/// use canopus_core::health::{TcpProbe, Probe};
/// use std::time::Duration;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let probe = TcpProbe::new("127.0.0.1", 8080, Duration::from_secs(5));
///
/// // This will fail unless something is listening on port 8080
/// match probe.check().await {
///     Ok(()) => println!("TCP connection successful"),
///     Err(e) => println!("TCP connection failed: {}", e),
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct TcpProbe {
    /// Target host to connect to
    host: String,
    /// Target port to connect to
    port: u16,
    /// Connection timeout
    timeout: Duration,
}

impl TcpProbe {
    /// Create a new TCP probe
    ///
    /// # Arguments
    ///
    /// * `host` - The host to connect to (e.g., "127.0.0.1", "localhost")
    /// * `port` - The port to connect to
    /// * `timeout` - Maximum time to wait for connection establishment
    pub fn new(host: impl Into<String>, port: u16, timeout: Duration) -> Self {
        Self {
            host: host.into(),
            port,
            timeout,
        }
    }

    /// Get the target address as a string
    #[must_use]
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[async_trait]
impl Probe for TcpProbe {
    async fn check(&self) -> Result<(), HealthError> {
        let address = self.address();
        debug!("TCP probe connecting to {}", address);

        match timeout(self.timeout, TcpStream::connect(&address)).await {
            Ok(Ok(_stream)) => {
                debug!("TCP probe to {} succeeded", address);
                // Stream is automatically dropped here, closing the connection
                Ok(())
            }
            Ok(Err(io_error)) => {
                debug!("TCP probe to {} failed: {}", address, io_error);
                Err(HealthError::Tcp(io_error))
            }
            Err(_timeout_error) => {
                debug!(
                    "TCP probe to {} timed out after {:?}",
                    address, self.timeout
                );
                Err(HealthError::Timeout(self.timeout))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::task;

    #[tokio::test]
    async fn test_tcp_probe_success() {
        // Bind to any available port
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let addr = listener.local_addr().expect("Failed to get local address");

        // Spawn a task to accept connections (but we don't need to do anything with them)
        let _handle = task::spawn(async move {
            while let Ok((_stream, _addr)) = listener.accept().await {
                // Just accept and drop connections
            }
        });

        let probe = TcpProbe::new("127.0.0.1", addr.port(), Duration::from_secs(1));
        let result = probe.check().await;
        assert!(result.is_ok(), "TCP probe should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_tcp_probe_connection_refused() {
        // Try to connect to a port that's definitely not listening
        let probe = TcpProbe::new("127.0.0.1", 1, Duration::from_secs(1));
        let result = probe.check().await;

        assert!(
            result.is_err(),
            "TCP probe should fail for refused connection"
        );
        match result.unwrap_err() {
            HealthError::Tcp(_) => {} // Expected
            other => panic!("Expected HealthError::Tcp, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_tcp_probe_timeout() {
        // Use a non-routable IP address to trigger timeout
        // 10.255.255.1 is non-routable and should timeout
        let probe = TcpProbe::new("10.255.255.1", 80, Duration::from_millis(100));
        let result = probe.check().await;

        assert!(result.is_err(), "TCP probe should timeout");
        match result.unwrap_err() {
            HealthError::Timeout(d) => assert_eq!(d, Duration::from_millis(100)),
            other => panic!("Expected HealthError::Timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_tcp_probe_address() {
        let probe = TcpProbe::new("localhost", 8080, Duration::from_secs(5));
        assert_eq!(probe.address(), "localhost:8080");

        let probe = TcpProbe::new("127.0.0.1", 3000, Duration::from_secs(1));
        assert_eq!(probe.address(), "127.0.0.1:3000");
    }
}
