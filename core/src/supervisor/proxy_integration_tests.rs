//! Tests for reverse proxy coupling with the supervisor

use crate::proxy::{MockProxyAdapter, ProxyOp};
use crate::supervisor::adapters::{MockInstruction, MockProcessAdapter};
use crate::supervisor::{spawn_supervisor, SupervisorConfig};
use schema::{
    BackoffConfig, HealthCheck, HealthCheckType, ReadinessCheck, RestartPolicy, ServiceEvent,
    ServiceSpec,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{timeout, Duration};

fn service_with_readiness(port: u16) -> ServiceSpec {
    ServiceSpec {
        id: "svc-proxy-test".to_string(),
        name: "Svc Proxy Test".to_string(),
        command: "sleep".to_string(),
        args: vec!["30".to_string()],
        environment: HashMap::default(),
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
        startup_timeout_secs: 10,
        route: Some("svc-proxy-test.local".to_string()),
    }
}

#[tokio::test]
async fn test_attach_on_ready_detach_on_stop() {
    // Start a TCP listener to satisfy readiness
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let _listener_task = tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    let spec = service_with_readiness(port);
    let process_adapter = Arc::new(MockProcessAdapter::new());
    process_adapter
        .add_instruction(MockInstruction {
            exit_delay: Duration::from_secs(30),
            exit_code: Some(0),
            signal: None,
            responds_to_signals: true,
        })
        .await;

    let proxy = Arc::new(MockProxyAdapter::new());
    let (event_tx, mut event_rx) = broadcast::channel(100);

    let config = SupervisorConfig {
        spec,
        process_adapter,
        event_tx,
        proxy_adapter: proxy.clone(),
    };

    let handle = spawn_supervisor(config);
    handle.start().unwrap();

    // Collect events for a short window
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(Ok(evt)) = timeout(Duration::from_millis(200), event_rx.recv()).await {
            events.push(evt);
            if let ServiceEvent::StateChanged { to_state, .. } = events.last().unwrap() {
                if to_state.is_ready() {
                    break;
                }
            }
        } else {
            break;
        }
    }
    let became_ready = events
        .iter()
        .any(|e| matches!(e, ServiceEvent::StateChanged { to_state, .. } if to_state.is_ready()));
    assert!(became_ready, "Service should become Ready");

    // Verify one attach op with correct host/port and correct event ordering: RouteAttached before Ready
    let ops = proxy.ops().await;
    assert!(ops.iter().any(|op| matches!(op, ProxyOp::Attach { host, port: p } if host == "svc-proxy-test.local" && *p == port)));
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, ProxyOp::Attach { .. }))
            .count(),
        1,
        "Attach should be called once"
    );
    let idx_attached = events
        .iter()
        .position(|e| matches!(e, ServiceEvent::RouteAttached { .. }))
        .expect("RouteAttached should be emitted");
    let idx_ready = events
        .iter()
        .position(
            |e| matches!(e, ServiceEvent::StateChanged { to_state, .. } if to_state.is_ready()),
        )
        .expect("Ready state should be emitted");
    assert!(
        idx_attached < idx_ready,
        "RouteAttached should occur before Ready state change"
    );

    // Stop and expect a single detach
    handle.stop().unwrap();
    // Collect stop sequence events
    let mut stop_events = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if let Ok(Ok(evt)) = timeout(Duration::from_millis(200), event_rx.recv()).await {
            stop_events.push(evt);
        } else {
            break;
        }
    }
    let ops = proxy.ops().await;
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, ProxyOp::Detach { .. }))
            .count(),
        1,
        "Detach should be called once on stop"
    );
    let idx_detached = stop_events
        .iter()
        .position(|e| matches!(e, ServiceEvent::RouteDetached { .. }))
        .expect("RouteDetached should be emitted on stop");
    // The first stop-related state change could be Stopping or directly Idle depending on timing
    let idx_state = stop_events.iter().position(|e| matches!(e, ServiceEvent::StateChanged { to_state, .. } if matches!(to_state, schema::ServiceState::Stopping | schema::ServiceState::Idle))).expect("A state change to Stopping/Idle is expected on stop");
    assert!(
        idx_detached < idx_state,
        "RouteDetached should occur before Stopping/Idle state change on stop"
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_detach_on_unhealthy_restart() {
    // Start a TCP listener to satisfy readiness and initial health
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => { break; }
                Ok((stream, _)) = listener.accept() => { drop(stream); }
                else => { break; }
            }
        }
    });

    // Service becomes ready via TCP readiness, then health fails when we drop the listener
    let spec = ServiceSpec {
        id: "svc-proxy-unhealthy".to_string(),
        name: "Svc Proxy Unhealthy".to_string(),
        command: "sleep".to_string(),
        args: vec!["30".to_string()],
        environment: HashMap::default(),
        working_directory: None,
        restart_policy: RestartPolicy::Always,
        backoff_config: BackoffConfig::default(),
        readiness_check: Some(ReadinessCheck {
            check_type: HealthCheckType::Tcp { port },
            initial_delay_secs: 0,
            interval_secs: 1,
            timeout_secs: 2,
            success_threshold: 1,
        }),
        health_check: Some(HealthCheck {
            check_type: HealthCheckType::Tcp { port },
            interval_secs: 1,
            timeout_secs: 2,
            failure_threshold: 1,
            success_threshold: 1,
        }),
        graceful_timeout_secs: 1,
        startup_timeout_secs: 30,
        route: Some("svc-proxy-unhealthy.local".to_string()),
    };

    let process_adapter = Arc::new(MockProcessAdapter::new());
    process_adapter
        .add_instruction(MockInstruction {
            exit_delay: Duration::from_secs(30),
            exit_code: Some(0),
            signal: None,
            responds_to_signals: true,
        })
        .await;

    let proxy = Arc::new(MockProxyAdapter::new());
    let (event_tx, mut event_rx) = broadcast::channel(200);
    let config = SupervisorConfig {
        spec,
        process_adapter,
        event_tx,
        proxy_adapter: proxy.clone(),
    };
    let handle = spawn_supervisor(config);
    handle.start().unwrap();

    // Wait until Ready
    let mut ready_seen = false;
    for _ in 0..30 {
        if let Ok(Ok(ServiceEvent::StateChanged { to_state, .. })) =
            timeout(Duration::from_millis(250), event_rx.recv()).await
        {
            if to_state.is_ready() {
                ready_seen = true;
                break;
            }
        }
    }
    assert!(ready_seen, "Service should become Ready");

    // Ensure route was attached prior to inducing unhealthy
    {
        let ops = proxy.ops().await;
        assert!(
            ops.iter().any(|op| matches!(op, ProxyOp::Attach { .. })),
            "Route should be attached when Ready"
        );
    }

    // Flip health to unhealthy by shutting down the TCP server
    let _ = shutdown_tx.send(());
    let _ = server_task.await;

    // Proactively trigger health checks until one fails
    let mut unhealthy_seen = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if matches!(
            timeout(Duration::from_secs(1), handle.trigger_health_check()).await,
            Ok(Ok(false))
        ) {
            unhealthy_seen = true;
            break;
        }
    }
    assert!(
        unhealthy_seen,
        "triggered health check should observe unhealthy"
    );

    // Wait for a restart event and verify proxy saw a Detach operation
    let mut saw_restart = false;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(8) {
        if let Ok(Ok(e)) = timeout(Duration::from_millis(500), event_rx.recv()).await {
            if matches!(e, ServiceEvent::ProcessStarted { .. }) {
                saw_restart = true;
                break;
            }
        }
    }
    assert!(saw_restart, "Should restart after unhealthy causes detach");
    let ops = proxy.ops().await;
    assert!(
        ops.iter().any(|op| matches!(op, ProxyOp::Detach { .. })),
        "Proxy should receive a Detach op on unhealthy"
    );

    handle.shutdown().unwrap();
}
