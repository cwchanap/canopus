//! Integration tests for supervisor health check functionality
//!
//! These tests verify that the supervisor correctly handles health checks,
//! readiness checks, startup timeouts, and related state transitions.

use crate::supervisor::{spawn_supervisor, SupervisorConfig};
use crate::supervisor::adapters::{MockProcessAdapter, MockInstruction};
use schema::{
    ServiceSpec, ServiceEvent, ServiceState, RestartPolicy, BackoffConfig, 
    HealthCheck, ReadinessCheck, HealthCheckType
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot};
use tokio::time::{timeout, sleep};
use tokio::net::TcpListener;
use hyper::{Body, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};

/// Simple HTTP server for testing health checks
pub(crate) struct TestHttpServer {
    port: u16,
    shutdown_tx: Option<oneshot::Sender<()>>,
    /// Controls whether health endpoint responds with 200 or 500
    healthy: Arc<AtomicBool>,
    /// Counter for how many times health endpoint was called
    health_calls: Arc<AtomicU32>,
    /// Controls whether readiness endpoint responds with 200 or 500
    ready: Arc<AtomicBool>,
    /// Counter for how many times readiness endpoint was called
    readiness_calls: Arc<AtomicU32>,
}

impl TestHttpServer {
    /// Start a new test HTTP server on an available port
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        
        let healthy = Arc::new(AtomicBool::new(true));
        let health_calls = Arc::new(AtomicU32::new(0));
        let ready = Arc::new(AtomicBool::new(true));
        let readiness_calls = Arc::new(AtomicU32::new(0));
        
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        
        let healthy_clone = healthy.clone();
        let health_calls_clone = health_calls.clone();
        let ready_clone = ready.clone();
        let readiness_calls_clone = readiness_calls.clone();
        
        // Service function to handle requests
        let make_svc = make_service_fn(move |_conn| {
            let healthy = healthy_clone.clone();
            let health_calls = health_calls_clone.clone();
            let ready = ready_clone.clone();
            let readiness_calls = readiness_calls_clone.clone();
            
            async move {
                Ok::<_, hyper::Error>(service_fn(move |req| {
                    let healthy = healthy.clone();
                    let health_calls = health_calls.clone();
                    let ready = ready.clone();
                    let readiness_calls = readiness_calls.clone();
                    
                    async move {
                        match req.uri().path() {
                            "/health" => {
                                health_calls.fetch_add(1, Ordering::SeqCst);
                                let response: Result<Response<Body>, hyper::Error> = if healthy.load(Ordering::SeqCst) {
                                    Ok(Response::new(Body::from("OK")))
                                } else {
                                    Ok(Response::builder()
                                        .status(500)
                                        .body(Body::from("Unhealthy"))
                                        .unwrap())
                                };
                                response
                            }
                            "/ready" => {
                                readiness_calls.fetch_add(1, Ordering::SeqCst);
                                if ready.load(Ordering::SeqCst) {
                                    Ok(Response::new(Body::from("Ready")))
                                } else {
                                    Ok(Response::builder()
                                        .status(503)
                                        .body(Body::from("Not ready"))
                                        .unwrap())
                                }
                            }
                            _ => {
                                Ok(Response::builder()
                                    .status(404)
                                    .body(Body::from("Not found"))
                                    .unwrap())
                            }
                        }
                    }
                }))
            }
        });
        
        // Start the server
        tokio::spawn(async move {
            let server = Server::from_tcp(listener.into_std().unwrap())
                .unwrap()
                .serve(make_svc)
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                });
                
            if let Err(e) = server.await {
                eprintln!("Server error: {}", e);
            }
        });
        
        // Give the server a moment to start
        sleep(Duration::from_millis(10)).await;
        
        Self {
            port,
            shutdown_tx: Some(shutdown_tx),
            healthy,
            health_calls,
            ready,
            readiness_calls,
        }
    }
    
    /// Get the port the server is listening on
    pub(crate) fn port(&self) -> u16 {
        self.port
    }
    
    /// Set whether the health endpoint should return healthy (200) or unhealthy (500)
    pub(crate) fn set_healthy(&self, healthy: bool) {
        self.healthy.store(healthy, Ordering::SeqCst);
    }
    
    /// Get the number of times the health endpoint was called
    pub(crate) fn health_call_count(&self) -> u32 {
        self.health_calls.load(Ordering::SeqCst)
    }
    
    /// Set whether the readiness endpoint should return ready (200) or not ready (503)
    pub(crate) fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }
    
    /// Get the number of times the readiness endpoint was called
    pub(crate) fn readiness_call_count(&self) -> u32 {
        self.readiness_calls.load(Ordering::SeqCst)
    }
    
    /// Shutdown the server
    pub(crate) fn shutdown(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }
}

fn create_http_server_spec(
    port: u16,
    with_readiness: bool,
    with_health: bool,
    startup_timeout_secs: u64
) -> ServiceSpec {
    let readiness_check = if with_readiness {
        Some(ReadinessCheck {
            check_type: HealthCheckType::Http {
                port,
                path: "/ready".to_string(),
                success_codes: vec![200],
            },
            initial_delay_secs: 0,
            interval_secs: 1,
            timeout_secs: 2,
            success_threshold: 1,
        })
    } else {
        None
    };
    
    let health_check = if with_health {
        Some(HealthCheck {
            check_type: HealthCheckType::Http {
                port,
                path: "/health".to_string(),
                success_codes: vec![200],
            },
            interval_secs: 2,
            timeout_secs: 2,
            failure_threshold: 2,
            success_threshold: 1,
        })
    } else {
        None
    };
    
    ServiceSpec {
        id: "health-test-service".to_string(),
        name: "Health Test Service".to_string(),
        command: "sleep".to_string(),
        args: vec!["3600".to_string()], // Sleep for a long time
        environment: Default::default(),
        working_directory: None,
        restart_policy: RestartPolicy::Always,
        backoff_config: BackoffConfig::default(),
        health_check,
        readiness_check,
        graceful_timeout_secs: 1,
        startup_timeout_secs,
    }
}

#[allow(dead_code)]
async fn collect_events_with_filter<F>(
    mut event_rx: broadcast::Receiver<ServiceEvent>, 
    timeout_duration: Duration,
    filter: F
) -> Vec<ServiceEvent>
where 
    F: Fn(&ServiceEvent) -> bool,
{
    let mut events = Vec::new();
    
    loop {
        match timeout(timeout_duration, event_rx.recv()).await {
            Ok(Ok(event)) => {
                if filter(&event) {
                    events.push(event);
                }
            },
            Ok(Err(_)) => break, // Channel closed
            Err(_) => break, // Timeout
        }
    }
    
    events
}

async fn collect_events(mut event_rx: broadcast::Receiver<ServiceEvent>, timeout_duration: Duration) -> Vec<ServiceEvent> {
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    
    loop {
        let remaining = timeout_duration.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            break;
        }
        
        match timeout(remaining, event_rx.recv()).await {
            Ok(Ok(event)) => {
                println!("Received event: {:?}", event);
                events.push(event);
            },
            Ok(Err(e)) => {
                println!("Event channel closed: {:?}", e);
                break; // Channel closed
            },
            Err(_) => {
                println!("Timeout waiting for events after {} events", events.len());
                break; // Timeout
            }
        }
    }
    
    events
}

#[cfg(test)]
mod tests {
    use super::*;

#[tokio::test]
    async fn test_readiness_check_success() {
        let server = TestHttpServer::start().await;
        let port = server.port();
        println!("Started test server on port {}", port);
        
        let spec = create_http_server_spec(port, true, false, 30);
        println!("Created service spec: {:?}", spec.id);
        
        // Create a mock process that stays running
        let process_adapter = Arc::new(MockProcessAdapter::new());
        process_adapter.add_instruction(MockInstruction {
            exit_delay: Duration::from_secs(10), // Run for 10 seconds
            exit_code: Some(0),
            signal: None,
            responds_to_signals: true,
        }).await;
        
        let (event_tx, event_rx) = broadcast::channel(100);
        
        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };
        
        let handle = spawn_supervisor(config);
        handle.start().unwrap();
        println!("Started supervisor");
        
        // Wait for readiness check events
        let events = collect_events(event_rx, Duration::from_secs(3)).await;
        
        println!("Collected {} events:", events.len());
        for (i, event) in events.iter().enumerate() {
            println!("  Event {}: {:?}", i, event);
        }
        
        // Should see: StateChanged(Idle->Spawning), ProcessStarted, StateChanged(Spawning->Starting), 
        // ReadinessCheckResult(success), StateChanged(Starting->Ready)
        let readiness_events: Vec<_> = events.iter()
            .filter(|event| matches!(event, ServiceEvent::ReadinessCheckResult { .. }))
            .collect();
        
        let state_changes: Vec<_> = events.iter()
            .filter_map(|event| match event {
                ServiceEvent::StateChanged { to_state, .. } => Some(*to_state),
                _ => None,
            })
            .collect();
        
        println!("Readiness events: {}, State changes: {:?}", readiness_events.len(), state_changes);
        println!("Server readiness calls: {}", server.readiness_call_count());
        
        // Should see at least one successful readiness check
        assert!(!readiness_events.is_empty(), "Should see readiness check events");
        
        // Should see transition to Ready state
        assert!(state_changes.contains(&ServiceState::Starting), "Should transition to Starting state");
        assert!(state_changes.contains(&ServiceState::Ready), "Should transition to Ready state after successful readiness check");
        
        // Verify server was actually called
        assert!(server.readiness_call_count() > 0, "Readiness endpoint should have been called");
        
        handle.shutdown().unwrap();
        server.shutdown();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_readiness_check_failure_then_success() {
        let server = TestHttpServer::start().await;
        let port = server.port();
        
        // Start with server not ready
        server.set_ready(false);
        
        let spec = create_http_server_spec(port, true, false, 30);
        
        let process_adapter = Arc::new(MockProcessAdapter::new());
        process_adapter.add_instruction(MockInstruction {
            exit_delay: Duration::from_secs(10),
            exit_code: Some(0),
            signal: None,
            responds_to_signals: true,
        }).await;
        
        let (event_tx, event_rx) = broadcast::channel(100);
        
        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };
        
        let handle = spawn_supervisor(config);
        handle.start().unwrap();
        
        // Wait a bit for initial failed readiness checks
        sleep(Duration::from_millis(500)).await;
        
        // Make server ready
        server.set_ready(true);
        
        // Wait for successful readiness check and transition to Ready
        let events = collect_events(event_rx, Duration::from_secs(3)).await;
        
        let readiness_events: Vec<_> = events.iter()
            .filter_map(|event| match event {
                ServiceEvent::ReadinessCheckResult { success, .. } => Some(*success),
                _ => None,
            })
            .collect();
        
        let state_changes: Vec<_> = events.iter()
            .filter_map(|event| match event {
                ServiceEvent::StateChanged { to_state, .. } => Some(*to_state),
                _ => None,
            })
            .collect();
        
        // Should see both failed and successful readiness checks
        assert!(readiness_events.contains(&false), "Should see failed readiness checks");
        assert!(readiness_events.contains(&true), "Should see successful readiness check");
        
        // Should eventually transition to Ready
        assert!(state_changes.contains(&ServiceState::Ready), "Should eventually transition to Ready state");
        
        // Should have made multiple calls to readiness endpoint
        assert!(server.readiness_call_count() > 1, "Should have made multiple readiness check calls");
        
        handle.shutdown().unwrap();
        server.shutdown();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test] 
    async fn test_health_check_after_ready() {
        let server = TestHttpServer::start().await;
        let port = server.port();
        
        let spec = create_http_server_spec(port, true, true, 30);
        
        let process_adapter = Arc::new(MockProcessAdapter::new());
        process_adapter.add_instruction(MockInstruction {
            exit_delay: Duration::from_secs(10),
            exit_code: Some(0),
            signal: None,
            responds_to_signals: true,
        }).await;
        
        let (event_tx, event_rx) = broadcast::channel(100);
        
        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };
        
        let handle = spawn_supervisor(config);
        handle.start().unwrap();
        
        // Wait for service to become ready and then for health checks
        let events = collect_events(event_rx, Duration::from_secs(5)).await;
        
        let health_events: Vec<_> = events.iter()
            .filter(|event| matches!(event, ServiceEvent::HealthCheckResult { .. }))
            .collect();
        
        let state_changes: Vec<_> = events.iter()
            .filter_map(|event| match event {
                ServiceEvent::StateChanged { to_state, .. } => Some(*to_state),
                _ => None,
            })
            .collect();
        
        // Should see readiness checks first, then health checks after becoming Ready
        assert!(state_changes.contains(&ServiceState::Ready), "Should become Ready");
        assert!(!health_events.is_empty(), "Should see health check events after becoming Ready");
        
        // Both endpoints should have been called
        assert!(server.readiness_call_count() > 0, "Readiness endpoint should have been called");
        assert!(server.health_call_count() > 0, "Health endpoint should have been called");
        
        handle.shutdown().unwrap();
        server.shutdown();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_health_check_failure_triggers_restart() {
        let server = TestHttpServer::start().await;
        let port = server.port();
        
        let spec = create_http_server_spec(port, true, true, 30);
        
        let process_adapter = Arc::new(MockProcessAdapter::new());
        // Add multiple processes for restart scenario
        for _ in 0..3 {
            process_adapter.add_instruction(MockInstruction {
                exit_delay: Duration::from_secs(10),
                exit_code: Some(0),
                signal: None,
                responds_to_signals: true,
            }).await;
        }
        
        let (event_tx, event_rx) = broadcast::channel(100);
        
        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };
        
        let handle = spawn_supervisor(config);
        handle.start().unwrap();
        
        // Wait for service to become ready
        sleep(Duration::from_secs(2)).await;
        
        // Make health checks fail
        server.set_healthy(false);
        
        // Wait for health check failures and restart
        let events = collect_events(event_rx, Duration::from_secs(8)).await;
        
        let unhealthy_events: Vec<_> = events.iter()
            .filter(|event| matches!(event, ServiceEvent::ServiceUnhealthy { .. }))
            .collect();
        
        let process_starts = events.iter()
            .filter(|event| matches!(event, ServiceEvent::ProcessStarted { .. }))
            .count();
        
        // Should see unhealthy event and process restart
        assert!(!unhealthy_events.is_empty(), "Should see ServiceUnhealthy event after consecutive health check failures");
        assert!(process_starts >= 2, "Should see process restart after health check failures");
        
        handle.shutdown().unwrap();
        server.shutdown();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_startup_timeout() {
        let server = TestHttpServer::start().await;
        let port = server.port();
        
        // Short startup timeout
        let spec = create_http_server_spec(port, true, false, 1);
        
        // Make readiness check fail so startup timeout triggers
        server.set_ready(false);
        
        let process_adapter = Arc::new(MockProcessAdapter::new());
        for _ in 0..3 {
            process_adapter.add_instruction(MockInstruction {
                exit_delay: Duration::from_secs(10),
                exit_code: Some(0),
                signal: None,
                responds_to_signals: true,
            }).await;
        }
        
        let (event_tx, event_rx) = broadcast::channel(100);
        
        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };
        
        let handle = spawn_supervisor(config);
        handle.start().unwrap();
        
        // Wait for startup timeout and restart
        let events = collect_events(event_rx, Duration::from_secs(5)).await;
        
        let timeout_events: Vec<_> = events.iter()
            .filter(|event| matches!(event, ServiceEvent::StartupTimeout { .. }))
            .collect();
        
        let process_starts = events.iter()
            .filter(|event| matches!(event, ServiceEvent::ProcessStarted { .. }))
            .count();
        
        // Should see startup timeout event and process restart
        assert!(!timeout_events.is_empty(), "Should see StartupTimeout event when service doesn't become ready in time");
        assert!(process_starts >= 2, "Should see process restart after startup timeout");
        
        handle.shutdown().unwrap();
        server.shutdown();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_no_health_checks_immediate_ready() {
        // Service with no health or readiness checks should go straight to Ready
        let spec = create_http_server_spec(8080, false, false, 30); // Port doesn't matter since no checks
        
        let process_adapter = Arc::new(MockProcessAdapter::new());
        process_adapter.add_instruction(MockInstruction {
            exit_delay: Duration::from_secs(5),
            exit_code: Some(0),
            signal: None,
            responds_to_signals: true,
        }).await;
        
        let (event_tx, event_rx) = broadcast::channel(100);
        
        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };
        
        let handle = spawn_supervisor(config);
        handle.start().unwrap();
        
        // Should quickly transition to Ready without health checks
        let events = collect_events(event_rx, Duration::from_secs(2)).await;
        
        let state_changes: Vec<_> = events.iter()
            .filter_map(|event| match event {
                ServiceEvent::StateChanged { from_state, to_state, .. } => Some((*from_state, *to_state)),
                _ => None,
            })
            .collect();
        
        let health_or_readiness_events = events.iter()
            .filter(|event| matches!(event, 
                ServiceEvent::HealthCheckResult { .. } | 
                ServiceEvent::ReadinessCheckResult { .. }
            ))
            .count();
        
        // Should see direct transition from Spawning to Ready (skipping Starting state)
        assert!(state_changes.contains(&(ServiceState::Spawning, ServiceState::Ready)), 
               "Should transition directly from Spawning to Ready without health checks");
        
        // Should not see any health check events
        assert_eq!(health_or_readiness_events, 0, "Should not see any health check events");
        
        handle.shutdown().unwrap();
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_tcp_health_check() {
        // Start a TCP listener to test TCP health checks
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        
        // Keep the listener alive for the test
        let _listener_handle = tokio::spawn(async move {
            // Just accept and immediately drop connections
            loop {
                if let Ok((stream, _)) = listener.accept().await {
                    drop(stream);
                }
            }
        });
        
        let spec = ServiceSpec {
            id: "tcp-health-test".to_string(),
            name: "TCP Health Test".to_string(),
            command: "sleep".to_string(),
            args: vec!["10".to_string()],
            environment: Default::default(),
            working_directory: None,
            restart_policy: RestartPolicy::Never,
            backoff_config: BackoffConfig::default(),
            health_check: None,
            readiness_check: Some(ReadinessCheck {
                check_type: HealthCheckType::Tcp { port },
                initial_delay_secs: 0,
                interval_secs: 1,
                timeout_secs: 2,
                success_threshold: 1,
            }),
            graceful_timeout_secs: 1,
            startup_timeout_secs: 30,
        };
        
        let process_adapter = Arc::new(MockProcessAdapter::new());
        process_adapter.add_instruction(MockInstruction {
            exit_delay: Duration::from_secs(5),
            exit_code: Some(0),
            signal: None,
            responds_to_signals: true,
        }).await;
        
        let (event_tx, event_rx) = broadcast::channel(100);
        
        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };
        
        let handle = spawn_supervisor(config);
        handle.start().unwrap();
        
        // Wait for TCP readiness check to succeed
        let events = collect_events(event_rx, Duration::from_secs(3)).await;
        
        let readiness_events: Vec<_> = events.iter()
            .filter_map(|event| match event {
                ServiceEvent::ReadinessCheckResult { success, .. } => Some(*success),
                _ => None,
            })
            .collect();
        
        let state_changes: Vec<_> = events.iter()
            .filter_map(|event| match event {
                ServiceEvent::StateChanged { to_state, .. } => Some(*to_state),
                _ => None,
            })
            .collect();
        
        // Should see successful TCP readiness check
        assert!(readiness_events.contains(&true), "Should see successful TCP readiness check");
        assert!(state_changes.contains(&ServiceState::Ready), "Should transition to Ready state");
        
        handle.shutdown().unwrap();
        sleep(Duration::from_millis(100)).await;
    }
}
