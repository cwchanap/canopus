//! Simple demonstration of the supervisor functionality
//!
//! This shows the basic usage of the supervisor system we've just implemented.

#![allow(unused_crate_dependencies)]
#![allow(unused_imports)]

use canopus_core::proxy::NoopProxyAdapter;
use canopus_core::supervisor::{spawn_supervisor, MockProcessAdapter, SupervisorConfig};
use canopus_core::Result;
use schema::{BackoffConfig, RestartPolicy, ServiceSpec};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{timeout, Duration};
use tracing::info;

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<()> {
    // Initialize tracing
    canopus_core::utils::init_tracing("info")?;

    info!("🚀 Starting supervisor demo");

    // Create a test service specification
    let spec = ServiceSpec {
        id: "demo-service".to_string(),
        name: "Demo Service".to_string(),
        command: "echo".to_string(),
        args: vec!["Hello from supervisor!".to_string()],
        environment: HashMap::default(),
        working_directory: None,
        route: None,
        restart_policy: RestartPolicy::Never,
        backoff_config: BackoffConfig::default(),
        health_check: None,
        readiness_check: None,
        graceful_timeout_secs: 5,
        startup_timeout_secs: 10,
    };

    // Create event channel for monitoring
    let (event_tx, mut event_rx) = broadcast::channel(100);

    // Create supervisor configuration
    let config = SupervisorConfig {
        spec,
        process_adapter: Arc::new(MockProcessAdapter::new()),
        event_tx,
        proxy_adapter: Arc::new(NoopProxyAdapter),
    };

    // Spawn the supervisor
    info!("📋 Spawning supervisor...");
    let handle = spawn_supervisor(config);

    // Monitor events in a separate task
    let monitor_task = tokio::spawn(async move {
        info!("👁 Starting event monitor");
        while let Ok(event) = event_rx.recv().await {
            match event {
                schema::ServiceEvent::StateChanged {
                    service_id,
                    from_state,
                    to_state,
                    ..
                } => {
                    info!(
                        "🔄 Service '{}' state: {:?} → {:?}",
                        service_id, from_state, to_state
                    );
                }
                schema::ServiceEvent::ProcessStarted {
                    service_id,
                    pid,
                    command,
                    args,
                    ..
                } => {
                    info!(
                        "✅ Process started for '{}' (PID: {}, Command: {} {:?})",
                        service_id, pid, command, args
                    );
                }
                schema::ServiceEvent::ProcessExited {
                    service_id,
                    exit_info,
                } => {
                    info!(
                        "❌ Process exited for '{}' (PID: {}, Exit code: {:?})",
                        service_id, exit_info.pid, exit_info.exit_code
                    );
                }
                schema::ServiceEvent::ConfigurationUpdated {
                    service_id,
                    changed_fields,
                    ..
                } => {
                    info!(
                        "⚙ Configuration updated for '{}': {:?}",
                        service_id, changed_fields
                    );
                }
                _ => {}
            }
        }
    });

    // Demonstrate the service lifecycle
    info!("▶ Starting service...");
    handle.start()?;

    // Wait a bit to see the process lifecycle
    tokio::time::sleep(Duration::from_millis(200)).await;

    info!("🔄 Restarting service...");
    handle.restart()?;

    // Wait a bit more
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Update the service spec
    info!("⚙ Updating service configuration...");
    let mut new_spec = handle.spec.clone();
    new_spec.command = "ls".to_string();
    new_spec.args = vec!["-la".to_string()];
    handle.update_spec(new_spec)?;

    // Wait for the update to take effect
    tokio::time::sleep(Duration::from_millis(200)).await;

    info!("🛑 Stopping service...");
    handle.stop()?;

    // Wait for stop to complete
    tokio::time::sleep(Duration::from_millis(200)).await;

    info!("🔚 Shutting down supervisor...");
    handle.shutdown()?;

    // Wait for monitor task to finish
    if timeout(Duration::from_millis(500), monitor_task)
        .await
        .is_err()
    {
        info!("Monitor task timed out, that's OK");
    }

    info!("✨ Demo completed successfully!");

    Ok(())
}
