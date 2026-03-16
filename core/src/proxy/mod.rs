//! Reverse proxy adapter interface
//!
//! This module defines a minimal interface for coupling the supervisor to a
//! reverse proxy. The actual proxy engine can be implemented later; for now we
//! provide a `NoopProxyAdapter` and a `MockProxyAdapter` for tests.

use crate::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// Trait for integrating with a reverse proxy
#[async_trait]
pub trait ProxyAdapter: Send + Sync {
    /// Attach a `host` to `port` in the proxy
    async fn attach(&self, host: &str, port: u16) -> Result<()>;
    /// Detach a `host` from the proxy
    async fn detach(&self, host: &str) -> Result<()>;
}

/// A no-op proxy adapter used by default
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopProxyAdapter;

#[async_trait]
impl ProxyAdapter for NoopProxyAdapter {
    async fn attach(&self, _host: &str, _port: u16) -> Result<()> {
        Ok(())
    }
    async fn detach(&self, _host: &str) -> Result<()> {
        Ok(())
    }
}

/// Recorded proxy operations for testing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyOp {
    /// Attach a host to the given backend port
    Attach {
        /// Hostname to register in the proxy
        host: String,
        /// Backend port to route traffic to
        port: u16,
    },
    /// Detach a host from the proxy
    Detach {
        /// Hostname to remove from the proxy
        host: String,
    },
}

/// Mock proxy adapter that records operations for assertions in tests
#[derive(Debug, Clone, Default)]
pub struct MockProxyAdapter {
    ops: Arc<tokio::sync::Mutex<Vec<ProxyOp>>>,
}

impl MockProxyAdapter {
    /// Create a new `MockProxyAdapter` with no recorded operations
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Return a snapshot of all recorded proxy operations
    pub async fn ops(&self) -> Vec<ProxyOp> {
        self.ops.lock().await.clone()
    }
    /// Clear all recorded operations
    pub async fn clear(&self) {
        self.ops.lock().await.clear();
    }
}

#[async_trait]
impl ProxyAdapter for MockProxyAdapter {
    async fn attach(&self, host: &str, port: u16) -> Result<()> {
        self.ops.lock().await.push(ProxyOp::Attach {
            host: host.to_string(),
            port,
        });
        Ok(())
    }
    async fn detach(&self, host: &str) -> Result<()> {
        self.ops.lock().await.push(ProxyOp::Detach {
            host: host.to_string(),
        });
        Ok(())
    }
}

/// Generic adapter that delegates to a synchronous `ProxyApi` implementation
#[derive(Debug, Clone)]
pub struct ApiProxyAdapter<T: crate::proxy_api::ProxyApi + Send + Sync + 'static> {
    inner: Arc<T>,
}

impl<T: crate::proxy_api::ProxyApi + Send + Sync + 'static> ApiProxyAdapter<T> {
    /// Create a new wrapper around a proxy adapter implementation
    pub const fn new(inner: Arc<T>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<T: crate::proxy_api::ProxyApi + Send + Sync + 'static> ProxyAdapter for ApiProxyAdapter<T> {
    async fn attach(&self, host: &str, port: u16) -> Result<()> {
        self.inner.attach(host, port)
    }
    async fn detach(&self, host: &str) -> Result<()> {
        self.inner.detach(host)
    }
}

/// Type alias for the built-in `NullProxy` adapter
pub type NullProxyAdapter = ApiProxyAdapter<crate::proxy_api::NullProxy>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy_api::NullProxy;

    // ── NoopProxyAdapter ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn noop_adapter_attach_succeeds() {
        let adapter = NoopProxyAdapter;
        let result = adapter.attach("app.dev", 3000).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn noop_adapter_detach_succeeds() {
        let adapter = NoopProxyAdapter;
        let result = adapter.detach("app.dev").await;
        assert!(result.is_ok());
    }

    // ── MockProxyAdapter ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn mock_adapter_records_attach() {
        let adapter = MockProxyAdapter::new();
        adapter.attach("app.dev", 8080).await.unwrap();

        let ops = adapter.ops().await;
        assert_eq!(ops.len(), 1);
        assert_eq!(
            ops[0],
            ProxyOp::Attach {
                host: "app.dev".to_string(),
                port: 8080
            }
        );
    }

    #[tokio::test]
    async fn mock_adapter_records_detach() {
        let adapter = MockProxyAdapter::new();
        adapter.attach("app.dev", 8080).await.unwrap();
        adapter.detach("app.dev").await.unwrap();

        let ops = adapter.ops().await;
        assert_eq!(ops.len(), 2);
        assert_eq!(
            ops[1],
            ProxyOp::Detach {
                host: "app.dev".to_string()
            }
        );
    }

    #[tokio::test]
    async fn mock_adapter_clear_removes_all_ops() {
        let adapter = MockProxyAdapter::new();
        adapter.attach("a.dev", 1000).await.unwrap();
        adapter.attach("b.dev", 2000).await.unwrap();
        assert_eq!(adapter.ops().await.len(), 2);

        adapter.clear().await;
        assert!(adapter.ops().await.is_empty());
    }

    #[tokio::test]
    async fn mock_adapter_default_has_no_ops() {
        let adapter = MockProxyAdapter::default();
        assert!(adapter.ops().await.is_empty());
    }

    // ── ApiProxyAdapter wrapping NullProxy ────────────────────────────────────

    #[tokio::test]
    async fn api_proxy_adapter_attach_delegates_to_inner() {
        let null = Arc::new(NullProxy::new());
        let adapter = ApiProxyAdapter::new(null.clone());

        adapter.attach("svc.dev", 9000).await.unwrap();
        assert_eq!(null.call_count(), 1);
    }

    #[tokio::test]
    async fn api_proxy_adapter_detach_delegates_to_inner() {
        let null = Arc::new(NullProxy::new());
        let adapter = ApiProxyAdapter::new(null.clone());

        adapter.attach("svc.dev", 9000).await.unwrap();
        adapter.detach("svc.dev").await.unwrap();
        assert_eq!(null.call_count(), 2);
    }
}
