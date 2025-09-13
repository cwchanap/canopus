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
    Attach { host: String, port: u16 },
    /// Detach a host from the proxy
    Detach { host: String },
}

/// Mock proxy adapter that records operations for assertions in tests
#[derive(Debug, Clone, Default)]
pub struct MockProxyAdapter {
    ops: Arc<tokio::sync::Mutex<Vec<ProxyOp>>>,
}

impl MockProxyAdapter {
    pub fn new() -> Self { Self::default() }
    pub async fn ops(&self) -> Vec<ProxyOp> { self.ops.lock().await.clone() }
    pub async fn clear(&self) { self.ops.lock().await.clear(); }
}

#[async_trait]
impl ProxyAdapter for MockProxyAdapter {
    async fn attach(&self, host: &str, port: u16) -> Result<()> {
        self.ops.lock().await.push(ProxyOp::Attach { host: host.to_string(), port });
        Ok(())
    }
    async fn detach(&self, host: &str) -> Result<()> {
        self.ops.lock().await.push(ProxyOp::Detach { host: host.to_string() });
        Ok(())
    }
}

/// Generic adapter that delegates to a synchronous `ProxyApi` implementation
#[derive(Debug, Clone)]
pub struct ApiProxyAdapter<T: crate::proxy_api::ProxyApi + Send + Sync + 'static> {
    inner: Arc<T>,
}

impl<T: crate::proxy_api::ProxyApi + Send + Sync + 'static> ApiProxyAdapter<T> {
    /// Create a new adapter from an existing `ProxyApi` instance
    pub fn new(inner: Arc<T>) -> Self { Self { inner } }
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

/// Type alias for the built-in NullProxy adapter
pub type NullProxyAdapter = ApiProxyAdapter<crate::proxy_api::NullProxy>;
