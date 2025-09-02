//! HTTP request health probing

use async_trait::async_trait;
use hyper::{Body, Client, Method, Request, Uri};
use std::time::Duration;
use tokio::time::timeout;
use tracing::debug;

use super::{Expect, HealthError, Probe};

/// HTTP health probe that makes GET requests and validates responses
///
/// This probe makes HTTP GET requests to a specified URL and validates
/// the response according to the configured expectation (status code,
/// 2xx range, or body content).
///
/// # Example
///
/// ```rust
/// use canopus_core::health::{HttpProbe, Expect, Probe};
/// use std::time::Duration;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let probe = HttpProbe::new(
///     "http://127.0.0.1:8080/health".to_string(),
///     Expect::Status(200),
///     Duration::from_secs(5)
/// );
/// 
/// match probe.check().await {
///     Ok(()) => println!("HTTP probe successful"),
///     Err(e) => println!("HTTP probe failed: {}", e),
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct HttpProbe {
    /// URL to request
    url: String,
    /// Expected response criteria
    expect: Expect,
    /// Request timeout
    timeout: Duration,
}

impl HttpProbe {
    /// Create a new HTTP probe
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to make a GET request to
    /// * `expect` - The expectation for validating the response
    /// * `timeout` - Maximum time to wait for the request to complete
    pub fn new(url: String, expect: Expect, timeout: Duration) -> Self {
        Self {
            url,
            expect,
            timeout,
        }
    }

    /// Get the target URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Get the expected response criteria
    pub fn expect(&self) -> &Expect {
        &self.expect
    }
}

#[async_trait]
impl Probe for HttpProbe {
    async fn check(&self) -> Result<(), HealthError> {
        debug!("HTTP probe requesting {}", self.url);

        let client = Client::new();
        
        // Parse the URL
        let uri: Uri = self.url.parse()?;
        
        // Create the request
        let req = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Body::empty())?;

        // Execute the request with timeout
        let response = match timeout(self.timeout, client.request(req)).await {
            Ok(Ok(response)) => response,
            Ok(Err(hyper_error)) => {
                debug!("HTTP probe to {} failed: {}", self.url, hyper_error);
                return Err(HealthError::Http(hyper_error));
            }
            Err(_timeout_error) => {
                debug!("HTTP probe to {} timed out after {:?}", self.url, self.timeout);
                return Err(HealthError::Timeout(self.timeout));
            }
        };

        let status = response.status();
        debug!("HTTP probe to {} returned status {}", self.url, status);

        // Check if status matches expectation
        if !self.expect.matches_status(status.as_u16()) {
            return Err(HealthError::UnexpectedStatus(status.as_u16()));
        }

        // If we need to check the body, read it
        if let Expect::BodyContains(_) = &self.expect {
            let body_bytes = match timeout(self.timeout, hyper::body::to_bytes(response.into_body())).await {
                Ok(Ok(bytes)) => bytes,
                Ok(Err(hyper_error)) => {
                    debug!("HTTP probe body read failed: {}", hyper_error);
                    return Err(HealthError::Http(hyper_error));
                }
                Err(_timeout_error) => {
                    debug!("HTTP probe body read timed out after {:?}", self.timeout);
                    return Err(HealthError::Timeout(self.timeout));
                }
            };

            let body_str = String::from_utf8_lossy(&body_bytes);
            debug!("HTTP probe response body: {}", body_str);

            if !self.expect.matches_body(&body_str) {
                return Err(HealthError::BodyMismatch);
            }
        }

        debug!("HTTP probe to {} succeeded", self.url);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use hyper::{Response, Server, service::{make_service_fn, service_fn}};
    use tokio::task;

    // Helper function to start a test HTTP server
    async fn start_test_server() -> u16 {
        let make_svc = make_service_fn(|_conn| {
            async {
                Ok::<_, Infallible>(service_fn(|req| {
                    async move {
                        let path = req.uri().path();
                        match path {
                            "/health" => Ok::<_, Infallible>(Response::new(Body::from("healthy"))),
                            "/ok" => Ok::<_, Infallible>(Response::new(Body::from("alive"))),
                            "/bad" => {
                                let response = Response::builder()
                                    .status(500)
                                    .body(Body::from("error"))
                                    .unwrap();
                                Ok::<_, Infallible>(response)
                            }
                            _ => {
                                let response = Response::builder()
                                    .status(404)
                                    .body(Body::from("not found"))
                                    .unwrap();
                                Ok::<_, Infallible>(response)
                            }
                        }
                    }
                }))
            }
        });

        let addr = ([127, 0, 0, 1], 0).into();
        let server = Server::bind(&addr).serve(make_svc);
        let port = server.local_addr().port();

        // Spawn the server in a background task
        task::spawn(async move {
            if let Err(e) = server.await {
                eprintln!("Server error: {}", e);
            }
        });

        // Give the server a moment to start
        tokio::time::sleep(Duration::from_millis(10)).await;
        port
    }

    #[tokio::test]
    async fn test_http_probe_status_success() {
        let port = start_test_server().await;
        let url = format!("http://127.0.0.1:{}/health", port);
        
        let probe = HttpProbe::new(url, Expect::Status(200), Duration::from_secs(5));
        let result = probe.check().await;
        assert!(result.is_ok(), "HTTP probe should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_http_probe_any2xx_success() {
        let port = start_test_server().await;
        let url = format!("http://127.0.0.1:{}/health", port);
        
        let probe = HttpProbe::new(url, Expect::Any2xx, Duration::from_secs(5));
        let result = probe.check().await;
        assert!(result.is_ok(), "HTTP probe with Any2xx should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_http_probe_body_contains_success() {
        let port = start_test_server().await;
        let url = format!("http://127.0.0.1:{}/health", port);
        
        let probe = HttpProbe::new(url, Expect::BodyContains("healthy".to_string()), Duration::from_secs(5));
        let result = probe.check().await;
        assert!(result.is_ok(), "HTTP probe with body check should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_http_probe_unexpected_status() {
        let port = start_test_server().await;
        let url = format!("http://127.0.0.1:{}/bad", port);
        
        let probe = HttpProbe::new(url, Expect::Status(200), Duration::from_secs(5));
        let result = probe.check().await;
        
        assert!(result.is_err(), "HTTP probe should fail for unexpected status");
        match result.unwrap_err() {
            HealthError::UnexpectedStatus(500) => {}, // Expected
            other => panic!("Expected HealthError::UnexpectedStatus(500), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_http_probe_body_mismatch() {
        let port = start_test_server().await;
        let url = format!("http://127.0.0.1:{}/health", port);
        
        let probe = HttpProbe::new(url, Expect::BodyContains("error".to_string()), Duration::from_secs(5));
        let result = probe.check().await;
        
        assert!(result.is_err(), "HTTP probe should fail for body mismatch");
        match result.unwrap_err() {
            HealthError::BodyMismatch => {}, // Expected
            other => panic!("Expected HealthError::BodyMismatch, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_http_probe_timeout() {
        // Use a non-routable IP to trigger timeout
        let url = "http://10.255.255.1:80/health".to_string();
        let probe = HttpProbe::new(url, Expect::Status(200), Duration::from_millis(100));
        let result = probe.check().await;
        
        assert!(result.is_err(), "HTTP probe should timeout");
        match result.unwrap_err() {
            HealthError::Timeout(d) => assert_eq!(d, Duration::from_millis(100)),
            other => panic!("Expected HealthError::Timeout, got {:?}", other),
        }
    }

    #[test]
    fn test_http_probe_getters() {
        let url = "http://localhost:8080/health".to_string();
        let expect = Expect::Status(200);
        let probe = HttpProbe::new(url.clone(), expect.clone(), Duration::from_secs(5));
        
        assert_eq!(probe.url(), "http://localhost:8080/health");
        assert_eq!(probe.expect(), &Expect::Status(200));
    }
}
