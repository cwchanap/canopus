//! Proxy API interface and implementations
//!
//! This module defines the stable attach/detach interface for proxy operations.
//! It provides a trait for proxy implementations and a null implementation for testing.
//!
//! The interface is designed to be idempotent - calling attach or detach multiple times
//! with the same parameters should be safe and not cause errors.

use crate::Result;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

use dashmap::DashMap;
use http::header::{HeaderName, HeaderValue, HOST};
use hyper::client::HttpConnector;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Request, Response, Server, Uri};

/// Proxy API trait defining the interface for proxy operations
///
/// Implementations should ensure that:
/// - `attach` operations are idempotent - calling attach multiple times for the same host:port is safe
/// - `detach` operations are idempotent - calling detach multiple times for the same host is safe
/// - Operations are thread-safe when called concurrently
pub trait ProxyApi {
    /// Attach a proxy for the given host and port
    ///
    /// This operation should be idempotent - calling it multiple times with the same
    /// host and port should either succeed or be safely ignored.
    ///
    /// # Arguments
    /// * `host` - The hostname or IP address to proxy
    /// * `port` - The port number to proxy
    ///
    /// # Returns
    /// * `Ok(())` on success or if already attached
    /// * `Err(_)` if attachment fails due to an unrecoverable error
    fn attach(&self, host: &str, port: u16) -> Result<()>;

    /// Detach the proxy for the given host
    ///
    /// This operation should be idempotent - calling it multiple times with the same
    /// host should either succeed or be safely ignored.
    ///
    /// # Arguments
    /// * `host` - The hostname or IP address to stop proxying
    ///
    /// # Returns
    /// * `Ok(())` on success or if not currently attached
    /// * `Err(_)` if detachment fails due to an unrecoverable error
    fn detach(&self, host: &str) -> Result<()>;
}

/// Log entry for recording proxy API calls
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallLog {
    /// An attach operation was called
    Attach {
        /// The host that was attached
        host: String,
        /// The port that was attached
        port: u16,
    },
    /// A detach operation was called
    Detach {
        /// The host that was detached
        host: String,
    },
}

/// A null proxy implementation that records calls for testing
///
/// This implementation doesn't perform any actual proxy operations,
/// but records all calls made to it for testing and verification purposes.
#[derive(Debug, Default)]
pub struct NullProxy {
    calls: Mutex<Vec<CallLog>>,
    attachments: Mutex<std::collections::HashSet<String>>,
}

impl NullProxy {
    /// Create a new NullProxy instance
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            attachments: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Get a copy of all recorded calls
    ///
    /// This method is available for testing to verify the sequence of operations.
    #[cfg(test)]
    pub fn get_calls(&self) -> Vec<CallLog> {
        self.calls.lock().unwrap().clone()
    }

    /// Get the count of recorded calls
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// Clear all recorded calls and reset state
    ///
    /// This is useful for testing to ensure clean state between test cases.
    #[cfg(test)]
    pub fn reset(&self) {
        self.calls.lock().unwrap().clear();
        self.attachments.lock().unwrap().clear();
    }

    /// Check if a host is currently attached
    #[cfg(test)]
    pub fn is_attached(&self, host: &str) -> bool {
        self.attachments.lock().unwrap().contains(host)
    }

    /// Get all currently attached hosts
    #[cfg(test)]
    pub fn get_attachments(&self) -> Vec<String> {
        self.attachments.lock().unwrap().iter().cloned().collect()
    }
}

impl ProxyApi for NullProxy {
    fn attach(&self, host: &str, port: u16) -> Result<()> {
        info!("NullProxy: Attaching proxy for {}:{}", host, port);

        // Record the call
        {
            let mut calls = self.calls.lock().unwrap();
            calls.push(CallLog::Attach {
                host: host.to_string(),
                port,
            });
        }

        // Track attachment (for idempotency testing)
        {
            let mut attachments = self.attachments.lock().unwrap();
            attachments.insert(host.to_string());
        }

        debug!(
            "NullProxy: Successfully attached proxy for {}:{}",
            host, port
        );
        Ok(())
    }

    fn detach(&self, host: &str) -> Result<()> {
        info!("NullProxy: Detaching proxy for {}", host);

        // Record the call
        {
            let mut calls = self.calls.lock().unwrap();
            calls.push(CallLog::Detach {
                host: host.to_string(),
            });
        }

        // Remove from attachments (for idempotency testing)
        {
            let mut attachments = self.attachments.lock().unwrap();
            attachments.remove(host);
        }

        debug!("NullProxy: Successfully detached proxy for {}", host);
        Ok(())
    }
}

/// Lightweight local HTTP reverse proxy that listens on a configurable address
/// (default 127.0.0.1:9080) and routes requests by Host header to a backend
/// on 127.0.0.1:<port> as registered via `attach`.
#[derive(Debug)]
pub struct LocalReverseProxy {
    routes: Arc<DashMap<String, u16>>, // hostname (lowercased, no port) -> backend port
    listen_addr: SocketAddr,
}

impl LocalReverseProxy {
    /// Create and start a local reverse proxy bound to `listen` (e.g., "127.0.0.1:9080").
    /// If parsing fails, falls back to 127.0.0.1:9080.
    pub fn new(listen: &str) -> Self {
        let listen_addr: SocketAddr = listen
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], 9080)));
        let routes: Arc<DashMap<String, u16>> = Arc::new(DashMap::new());

        let routes_clone = routes.clone();
        let make_svc = make_service_fn(move |_conn| {
            let routes = routes_clone.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let routes = routes.clone();
                    async move { Self::handle_request(req, routes).await }
                }))
            }
        });

        // Spawn the HTTP server in the background
        tokio::spawn(async move {
            if let Err(e) = Server::bind(&listen_addr).serve(make_svc).await {
                warn!("LocalReverseProxy server error on {}: {}", listen_addr, e);
            }
        });

        info!("LocalReverseProxy listening on {}", listen_addr);
        Self {
            routes,
            listen_addr,
        }
    }

    fn normalize_host(host: &str) -> String {
        // Lowercase, strip trailing dot, and strip port suffix
        let mut h = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if let Some(idx) = h.rfind(':') {
            // Split only if the suffix is a valid u16 (port)
            if h[idx + 1..].parse::<u16>().is_ok() {
                h.truncate(idx);
            }
        }
        h
    }

    async fn handle_request(
        req: Request<Body>,
        routes: Arc<DashMap<String, u16>>,
    ) -> Result<Response<Body>> {
        // Determine host
        let host = match req.headers().get(HOST).and_then(|v| v.to_str().ok()) {
            Some(h) => h,
            None => {
                let mut resp = Response::new(Body::from("missing Host header"));
                *resp.status_mut() = http::StatusCode::BAD_REQUEST;
                return Ok(resp);
            }
        };
        let host_key = Self::normalize_host(host);
        let port = match routes.get(&host_key) {
            Some(entry) => *entry.value(),
            None => {
                let mut resp = Response::new(Body::from(format!(
                    "no route for host: {}",
                    host_key
                )));
                *resp.status_mut() = http::StatusCode::BAD_GATEWAY;
                return Ok(resp);
            }
        };

        // Build backend URI: http://127.0.0.1:<port><path_and_query>
        let path_q = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");
        let target = format!("http://127.0.0.1:{}{}", port, path_q);
        let uri: Uri = match target.parse() {
            Ok(u) => u,
            Err(e) => {
                let mut resp = Response::new(Body::from(format!("bad target uri: {}", e)));
                *resp.status_mut() = http::StatusCode::BAD_GATEWAY;
                return Ok(resp);
            }
        };

        // Construct new request preserving method, version, and most headers
        let (mut parts, body) = req.into_parts();
        parts.uri = uri;

        // Add/override some forwarding headers
        parts.headers.insert(
            HeaderName::from_static("x-forwarded-host"),
            HeaderValue::from_str(host).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        parts.headers.insert(
            HeaderName::from_static("x-forwarded-proto"),
            HeaderValue::from_static("http"),
        );

        let out_req = Request::from_parts(parts, body);

        let client: Client<HttpConnector, Body> = Client::new();
        match client.request(out_req).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                let mut resp = Response::new(Body::from(format!(
                    "upstream request failed: {}",
                    e
                )));
                *resp.status_mut() = http::StatusCode::BAD_GATEWAY;
                Ok(resp)
            }
        }
    }
}

impl ProxyApi for LocalReverseProxy {
    fn attach(&self, host: &str, port: u16) -> Result<()> {
        let key = Self::normalize_host(host);
        self.routes.insert(key.clone(), port);
        info!("LocalReverseProxy: attach {} -> {}", key, port);
        Ok(())
    }

    fn detach(&self, host: &str) -> Result<()> {
        let key = Self::normalize_host(host);
        self.routes.remove(&key);
        info!("LocalReverseProxy: detach {}", key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_proxy_creation() {
        let proxy = NullProxy::new();
        assert_eq!(proxy.call_count(), 0);
        assert_eq!(proxy.get_calls().len(), 0);
        assert_eq!(proxy.get_attachments().len(), 0);
    }

    #[test]
    fn test_attach_operation() {
        let proxy = NullProxy::new();

        let result = proxy.attach("example.com", 8080);
        assert!(result.is_ok());

        assert_eq!(proxy.call_count(), 1);
        let calls = proxy.get_calls();
        assert_eq!(calls.len(), 1);

        match &calls[0] {
            CallLog::Attach { host, port } => {
                assert_eq!(host, "example.com");
                assert_eq!(*port, 8080);
            }
            _ => panic!("Expected Attach call"),
        }

        assert!(proxy.is_attached("example.com"));
        assert_eq!(proxy.get_attachments(), vec!["example.com"]);
    }

    #[test]
    fn test_detach_operation() {
        let proxy = NullProxy::new();

        // First attach something
        proxy.attach("example.com", 8080).unwrap();
        assert!(proxy.is_attached("example.com"));

        // Then detach it
        let result = proxy.detach("example.com");
        assert!(result.is_ok());

        assert_eq!(proxy.call_count(), 2);
        let calls = proxy.get_calls();
        assert_eq!(calls.len(), 2);

        match &calls[1] {
            CallLog::Detach { host } => {
                assert_eq!(host, "example.com");
            }
            _ => panic!("Expected Detach call"),
        }

        assert!(!proxy.is_attached("example.com"));
        assert_eq!(proxy.get_attachments().len(), 0);
    }

    #[test]
    fn test_idempotent_attach() {
        let proxy = NullProxy::new();

        // Attach the same host:port multiple times
        proxy.attach("example.com", 8080).unwrap();
        proxy.attach("example.com", 8080).unwrap();
        proxy.attach("example.com", 8080).unwrap();

        // All calls should succeed
        assert_eq!(proxy.call_count(), 3);

        // Should still only be attached once
        assert!(proxy.is_attached("example.com"));
        assert_eq!(proxy.get_attachments(), vec!["example.com"]);

        // All calls should be recorded
        let calls = proxy.get_calls();
        assert_eq!(calls.len(), 3);
        for call in calls {
            match call {
                CallLog::Attach { host, port } => {
                    assert_eq!(host, "example.com");
                    assert_eq!(port, 8080);
                }
                _ => panic!("Expected only Attach calls"),
            }
        }
    }

    #[test]
    fn test_idempotent_detach() {
        let proxy = NullProxy::new();

        // Attach first
        proxy.attach("example.com", 8080).unwrap();
        assert!(proxy.is_attached("example.com"));

        // Detach multiple times
        proxy.detach("example.com").unwrap();
        proxy.detach("example.com").unwrap();
        proxy.detach("example.com").unwrap();

        // All detach calls should succeed
        assert_eq!(proxy.call_count(), 4);

        // Should not be attached anymore
        assert!(!proxy.is_attached("example.com"));
        assert_eq!(proxy.get_attachments().len(), 0);

        // Verify call sequence
        let calls = proxy.get_calls();
        assert_eq!(calls.len(), 4);

        // First should be attach
        match &calls[0] {
            CallLog::Attach { host, port } => {
                assert_eq!(host, "example.com");
                assert_eq!(*port, 8080);
            }
            _ => panic!("Expected Attach call"),
        }

        // Rest should be detach
        for call in &calls[1..] {
            match call {
                CallLog::Detach { host } => {
                    assert_eq!(host, "example.com");
                }
                _ => panic!("Expected Detach call"),
            }
        }
    }

    #[test]
    fn test_detach_without_attach() {
        let proxy = NullProxy::new();

        // Detach without attaching first - should still succeed (idempotent)
        let result = proxy.detach("nonexistent.com");
        assert!(result.is_ok());

        assert_eq!(proxy.call_count(), 1);
        assert!(!proxy.is_attached("nonexistent.com"));
    }

    #[test]
    fn test_multiple_hosts() {
        let proxy = NullProxy::new();

        // Attach multiple hosts
        proxy.attach("example.com", 8080).unwrap();
        proxy.attach("test.com", 9090).unwrap();
        proxy.attach("local.dev", 3000).unwrap();

        // All should be attached
        assert!(proxy.is_attached("example.com"));
        assert!(proxy.is_attached("test.com"));
        assert!(proxy.is_attached("local.dev"));
        assert_eq!(proxy.get_attachments().len(), 3);

        // Detach one
        proxy.detach("test.com").unwrap();

        // Others should still be attached
        assert!(proxy.is_attached("example.com"));
        assert!(!proxy.is_attached("test.com"));
        assert!(proxy.is_attached("local.dev"));
        assert_eq!(proxy.get_attachments().len(), 2);
    }

    #[test]
    fn test_reset_functionality() {
        let proxy = NullProxy::new();

        // Make some calls
        proxy.attach("example.com", 8080).unwrap();
        proxy.attach("test.com", 9090).unwrap();
        proxy.detach("example.com").unwrap();

        assert_eq!(proxy.call_count(), 3);
        assert_eq!(proxy.get_attachments().len(), 1);

        // Reset
        proxy.reset();

        // Should be clean state
        assert_eq!(proxy.call_count(), 0);
        assert_eq!(proxy.get_calls().len(), 0);
        assert_eq!(proxy.get_attachments().len(), 0);
        assert!(!proxy.is_attached("test.com"));
    }

    #[test]
    fn test_golden_sequence() {
        let proxy = NullProxy::new();

        // Execute a specific sequence of operations
        proxy.attach("api.example.com", 8080).unwrap();
        proxy.attach("web.example.com", 3000).unwrap();
        proxy.detach("api.example.com").unwrap();
        proxy.attach("api.example.com", 8081).unwrap(); // Same host, different port
        proxy.detach("web.example.com").unwrap();
        proxy.detach("api.example.com").unwrap();

        // Verify the exact sequence was recorded
        let calls = proxy.get_calls();
        let expected = vec![
            CallLog::Attach {
                host: "api.example.com".to_string(),
                port: 8080,
            },
            CallLog::Attach {
                host: "web.example.com".to_string(),
                port: 3000,
            },
            CallLog::Detach {
                host: "api.example.com".to_string(),
            },
            CallLog::Attach {
                host: "api.example.com".to_string(),
                port: 8081,
            },
            CallLog::Detach {
                host: "web.example.com".to_string(),
            },
            CallLog::Detach {
                host: "api.example.com".to_string(),
            },
        ];

        assert_eq!(calls, expected);

        // Final state should have no attachments
        assert_eq!(proxy.get_attachments().len(), 0);
    }

    #[test]
    fn test_call_log_debug_format() {
        let attach_log = CallLog::Attach {
            host: "example.com".to_string(),
            port: 8080,
        };
        let detach_log = CallLog::Detach {
            host: "example.com".to_string(),
        };

        // Just verify that Debug formatting works without panicking
        let _attach_debug = format!("{:?}", attach_log);
        let _detach_debug = format!("{:?}", detach_log);
    }
}
