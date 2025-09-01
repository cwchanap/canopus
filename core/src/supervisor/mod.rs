//! Service supervisor implementation
//!
//! This module provides the core supervisor functionality for managing service
//! lifecycles, including process spawning, health checking, restart policies,
//! and event broadcasting.
//!
//! ## Architecture
//!
//! The supervisor uses a per-service task model where each service gets its own
//! tokio task that manages the service's lifecycle through state transitions:
//!
//! ```text
//! Idle → Spawning → Starting → Ready → Stopping → Idle
//! ```
//!
//! ## Components
//!
//! - [`SupervisorHandle`]: Control interface for supervisor operations
//! - [`ControlMsg`]: Messages for controlling service lifecycle
//! - [`ProcessAdapter`]: Trait for abstracting process management
//! - [`ServiceSupervisor`]: Per-service task managing state transitions

use crate::Result;
use schema::{ServiceEvent, ServiceSpec, ServiceState};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{error, info};

pub mod adapters;
pub mod service_task;

pub use adapters::*;
pub use service_task::*;

/// Control messages for supervisor operations
#[derive(Debug, Clone)]
pub enum ControlMsg {
    /// Start the service
    Start,
    /// Stop the service gracefully
    Stop,
    /// Restart the service (stop then start)
    Restart,
    /// Update the service specification
    UpdateSpec(ServiceSpec),
    /// Shutdown the supervisor (stop service and terminate task)
    Shutdown,
}

/// Current internal state of the supervisor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InternalState {
    /// Service is not running
    Idle,
    /// Service process is being started
    Spawning,
    /// Service is running but not yet ready (health checks pending)
    Starting,
    /// Service is running and healthy
    Ready,
    /// Service is being gracefully terminated
    Stopping,
}

impl From<InternalState> for ServiceState {
    fn from(state: InternalState) -> Self {
        match state {
            InternalState::Idle => ServiceState::Idle,
            InternalState::Spawning => ServiceState::Spawning,
            InternalState::Starting => ServiceState::Starting,
            InternalState::Ready => ServiceState::Ready,
            InternalState::Stopping => ServiceState::Stopping,
        }
    }
}

/// Handle for controlling a supervisor instance
#[derive(Debug, Clone)]
pub struct SupervisorHandle {
    /// Service specification
    pub spec: ServiceSpec,
    /// Channel for sending control messages
    control_tx: mpsc::UnboundedSender<ControlMsg>,
    /// Receiver for state updates
    state_rx: watch::Receiver<ServiceState>,
}

impl SupervisorHandle {
    /// Send a control message to the supervisor
    pub fn send(&self, msg: ControlMsg) -> Result<()> {
        self.control_tx
            .send(msg)
            .map_err(|_| crate::CoreError::ServiceError("Supervisor task has shut down".to_string()))?;
        Ok(())
    }

    /// Start the service
    pub fn start(&self) -> Result<()> {
        self.send(ControlMsg::Start)
    }

    /// Stop the service
    pub fn stop(&self) -> Result<()> {
        self.send(ControlMsg::Stop)
    }

    /// Restart the service
    pub fn restart(&self) -> Result<()> {
        self.send(ControlMsg::Restart)
    }

    /// Update the service specification
    pub fn update_spec(&self, spec: ServiceSpec) -> Result<()> {
        self.send(ControlMsg::UpdateSpec(spec))
    }

    /// Shutdown the supervisor
    pub fn shutdown(&self) -> Result<()> {
        self.send(ControlMsg::Shutdown)
    }

    /// Get the current state of the service
    pub fn current_state(&self) -> ServiceState {
        *self.state_rx.borrow()
    }

    /// Subscribe to state changes
    pub fn subscribe_to_state(&self) -> watch::Receiver<ServiceState> {
        self.state_rx.clone()
    }
}

/// Configuration for spawning a supervisor
pub struct SupervisorConfig {
    /// Service specification
    pub spec: ServiceSpec,
    /// Process adapter for spawning and managing processes
    pub process_adapter: Arc<dyn ProcessAdapter>,
    /// Event broadcaster for emitting service events
    pub event_tx: broadcast::Sender<ServiceEvent>,
}

/// Spawn a supervisor for the given service specification
///
/// This creates a new tokio task that manages the service lifecycle according
/// to the provided specification. The supervisor will emit events to the
/// provided broadcast channel and use the process adapter for actual process management.
///
/// # Arguments
///
/// * `config` - Configuration for the supervisor
///
/// # Returns
///
/// A [`SupervisorHandle`] that can be used to control the supervisor.
pub fn spawn_supervisor(config: SupervisorConfig) -> SupervisorHandle {
    let SupervisorConfig {
        spec,
        process_adapter,
        event_tx,
    } = config;

    let (control_tx, control_rx) = mpsc::unbounded_channel();
    let (state_tx, state_rx) = watch::channel(ServiceState::Idle);

    info!("Spawning supervisor for service '{}'", spec.id);

    // Clone spec for the handle
    let handle_spec = spec.clone();

    // Spawn the supervisor task
    let service_id = spec.id.clone();
    tokio::spawn(async move {
        let mut supervisor = ServiceSupervisor::new(spec, process_adapter, event_tx, state_tx);

        if let Err(e) = supervisor.run(control_rx).await {
            error!("Supervisor task for service '{}' failed: {}", service_id, e);
        }

        info!("Supervisor task for service '{}' terminated", service_id);
    });

    SupervisorHandle {
        spec: handle_spec,
        control_tx,
        state_rx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::adapters::MockProcessAdapter;
    use schema::RestartPolicy;
    use std::time::Duration;
    use tokio::time::timeout;

    fn create_test_spec() -> ServiceSpec {
        ServiceSpec {
            id: "test-service".to_string(),
            name: "Test Service".to_string(),
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            environment: Default::default(),
            working_directory: None,
            restart_policy: RestartPolicy::Never,
            backoff_config: Default::default(),
            health_check: None,
            readiness_check: None,
            graceful_timeout_secs: 5,
            startup_timeout_secs: 10,
        }
    }

    #[tokio::test]
    async fn test_supervisor_spawn() {
        let spec = create_test_spec();
        let process_adapter = Arc::new(MockProcessAdapter::new());
        let (event_tx, mut event_rx) = broadcast::channel(100);

        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };

        let handle = spawn_supervisor(config);

        // Initial state should be Idle
        assert_eq!(handle.current_state(), ServiceState::Idle);

        // Should be able to send control messages
        assert!(handle.start().is_ok());

        // Give the task time to process the message
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should receive a state change event
        if let Ok(event) = timeout(Duration::from_millis(100), event_rx.recv()).await {
            match event.unwrap() {
                ServiceEvent::StateChanged { from_state, to_state, .. } => {
                    assert_eq!(from_state, ServiceState::Idle);
                    assert!(matches!(to_state, ServiceState::Spawning));
                }
                _ => panic!("Expected StateChanged event"),
            }
        }

        // Clean shutdown
        assert!(handle.shutdown().is_ok());
        
        // Give time for shutdown
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_supervisor_handle_operations() {
        let spec = create_test_spec();
        let process_adapter = Arc::new(MockProcessAdapter::new());
        let (event_tx, _event_rx) = broadcast::channel(100);

        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };

        let handle = spawn_supervisor(config);

        // Test all control operations
        assert!(handle.start().is_ok());
        assert!(handle.stop().is_ok());
        assert!(handle.restart().is_ok());
        
        let new_spec = create_test_spec();
        assert!(handle.update_spec(new_spec).is_ok());
        
        assert!(handle.shutdown().is_ok());
        
        // Give time for shutdown
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_supervisor_state_subscription() {
        let spec = create_test_spec();
        let process_adapter = Arc::new(MockProcessAdapter::new());
        let (event_tx, _event_rx) = broadcast::channel(100);

        let config = SupervisorConfig {
            spec,
            process_adapter,
            event_tx,
        };

        let handle = spawn_supervisor(config);
        let mut state_rx = handle.subscribe_to_state();

        // Initial state
        assert_eq!(*state_rx.borrow(), ServiceState::Idle);

        // Start service and watch for state change
        handle.start().unwrap();

        // Wait for any state changes - we expect at least one transition away from Idle
        let mut saw_non_idle = false;
        for _ in 0..5 {
            if timeout(Duration::from_millis(100), state_rx.changed()).await.is_ok() {
                let new_state = *state_rx.borrow();
                if new_state != ServiceState::Idle {
                    saw_non_idle = true;
                    break;
                }
            }
        }
        
        // We should have seen at least one non-idle state during the service lifecycle
        assert!(saw_non_idle, "Should have observed at least one non-idle state transition");

        handle.shutdown().unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
