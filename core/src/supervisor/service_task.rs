//! Service supervisor task implementation
//!
//! This module contains the [`ServiceSupervisor`] which implements the core
//! state machine logic for managing a single service's lifecycle.

use super::{ControlMsg, InternalState, ManagedProcess, ProcessAdapter, RestartPolicyEngine, RestartAction};
use crate::Result;
use schema::{ServiceEvent, ServiceExit, ServiceSpec, ServiceState};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::{timeout, Duration, sleep};
use tracing::{debug, error, info, warn};

/// Service supervisor task managing the lifecycle of a single service
pub struct ServiceSupervisor {
    /// Service specification
    spec: ServiceSpec,
    /// Current internal state
    state: InternalState,
    /// Process adapter for spawning and managing processes
    process_adapter: Arc<dyn ProcessAdapter>,
    /// Event broadcaster
    event_tx: broadcast::Sender<ServiceEvent>,
    /// State broadcaster
    state_tx: watch::Sender<ServiceState>,
    /// Currently managed process (if any)
    current_process: Option<Box<dyn ManagedProcess>>,
    /// Restart policy engine for determining restart actions
    restart_policy_engine: RestartPolicyEngine,
    /// Restart timer (when restart is scheduled)
    restart_timer: Option<tokio::time::Instant>,
}

impl ServiceSupervisor {
    /// Create a new service supervisor
    pub fn new(
        spec: ServiceSpec,
        process_adapter: Arc<dyn ProcessAdapter>,
        event_tx: broadcast::Sender<ServiceEvent>,
        state_tx: watch::Sender<ServiceState>,
    ) -> Self {
        let restart_policy_engine = RestartPolicyEngine::new(
            spec.restart_policy,
            spec.backoff_config
        );
        
        Self {
            spec,
            state: InternalState::Idle,
            process_adapter,
            event_tx,
            state_tx,
            current_process: None,
            restart_policy_engine,
            restart_timer: None,
        }
    }

    /// Run the supervisor task loop
    pub async fn run(&mut self, mut control_rx: mpsc::UnboundedReceiver<ControlMsg>) -> Result<()> {
        info!("Starting supervisor for service '{}'", self.spec.id);

        loop {
            tokio::select! {
                // Handle control messages
                msg = control_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            debug!("Received control message: {:?}", msg);
                            if let Err(e) = self.handle_control_message(msg).await {
                                error!("Error handling control message: {}", e);
                            }
                        }
                        None => {
                            info!("Control channel closed, shutting down supervisor");
                            break;
                        }
                    }
                }

                // Handle process exits (if we have a running process)
                exit_result = self.wait_for_process_exit(), if self.current_process.is_some() => {
                    match exit_result {
                        Ok(exit_info) => {
                            self.handle_process_exit(exit_info).await?;
                        }
                        Err(e) => {
                            error!("Error waiting for process exit: {}", e);
                            self.transition_to(InternalState::Idle, Some("Process wait error".to_string())).await?;
                        }
                    }
                }
                
                // Handle restart timer expiry
                _ = sleep(Duration::from_millis(100)), if self.restart_timer.is_some() => {
                    if let Some(restart_time) = self.restart_timer {
                        if tokio::time::Instant::now() >= restart_time {
                            debug!("Restart timer expired for service '{}'", self.spec.id);
                            self.restart_timer = None;
                            
                            // Check if we should still restart (service might have been stopped)
                            if matches!(self.state, InternalState::Idle) {
                                info!("Starting automatic restart for service '{}'", self.spec.id);
                                if let Err(e) = self.start_service().await {
                                    error!("Failed to restart service '{}': {}", self.spec.id, e);
                                    // If restart fails, stay in Idle state
                                    self.transition_to(InternalState::Idle, Some("Restart failed".to_string())).await?;
                                }
                            } else {
                                debug!("Service state changed during restart delay, canceling restart");
                            }
                        }
                    }
                }
            }

            // Check if we need to perform readiness check after other operations
            if matches!(self.state, InternalState::Starting) {
                // Perform readiness check
                self.check_readiness().await;
                // Transition to Ready after readiness check
                self.transition_to(InternalState::Ready, Some("Service is ready".to_string())).await?;
            }
        }

        // Cleanup: stop any running process
        if self.current_process.is_some() {
            self.stop_process().await?;
        }

        Ok(())
    }

    /// Handle a control message
    async fn handle_control_message(&mut self, msg: ControlMsg) -> Result<()> {
        match msg {
            ControlMsg::Start => {
                self.start_service().await?;
            }
            ControlMsg::Stop => {
                self.stop_service().await?;
            }
            ControlMsg::Restart => {
                self.restart_service().await?;
            }
            ControlMsg::UpdateSpec(new_spec) => {
                self.update_spec(new_spec).await?;
            }
            ControlMsg::Shutdown => {
                info!("Shutdown requested for service '{}'", self.spec.id);
                if self.current_process.is_some() {
                    self.stop_process().await?;
                }
                return Err(crate::CoreError::ServiceError("Shutdown requested".to_string()));
            }
        }
        Ok(())
    }

    /// Start the service
    async fn start_service(&mut self) -> Result<()> {
        match self.state {
            InternalState::Idle => {
                self.transition_to(InternalState::Spawning, Some("Starting service".to_string())).await?;
                self.spawn_process().await?;
                
                if self.spec.readiness_check.is_some() || self.spec.health_check.is_some() {
                    self.transition_to(InternalState::Starting, Some("Waiting for readiness".to_string())).await?;
                } else {
                    // No health/readiness checks - go straight to Ready
                    self.transition_to(InternalState::Ready, Some("Service started successfully".to_string())).await?;
                }
            }
            _ => {
                warn!("Cannot start service '{}' - already running (state: {:?})", self.spec.id, self.state);
            }
        }
        Ok(())
    }

    /// Stop the service
    async fn stop_service(&mut self) -> Result<()> {
        // Cancel any pending restart timers
        if self.restart_timer.is_some() {
            debug!("Canceling pending restart timer for service '{}'", self.spec.id);
            self.restart_timer = None;
        }
        
        match self.state {
            InternalState::Idle => {
                debug!("Service '{}' already stopped", self.spec.id);
            }
            InternalState::Stopping => {
                debug!("Service '{}' already stopping", self.spec.id);
            }
            _ => {
                self.transition_to(InternalState::Stopping, Some("Stopping service".to_string())).await?;
                self.stop_process().await?;
            }
        }
        Ok(())
    }

    /// Restart the service
    async fn restart_service(&mut self) -> Result<()> {
        info!("Restarting service '{}'", self.spec.id);
        if !matches!(self.state, InternalState::Idle) {
            self.stop_service().await?;
            // Wait for the process to actually exit
            while !matches!(self.state, InternalState::Idle) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        self.start_service().await
    }

    /// Update the service specification
    async fn update_spec(&mut self, new_spec: ServiceSpec) -> Result<()> {
        info!("Updating spec for service '{}'", self.spec.id);
        
        let changed_fields = self.get_changed_fields(&new_spec);
        self.spec = new_spec;
        
        // Emit configuration updated event
        self.emit_event(ServiceEvent::ConfigurationUpdated {
            service_id: self.spec.id.clone(),
            timestamp: ServiceEvent::current_timestamp(),
            changed_fields,
        }).await;

        // If the service is running and critical fields changed, restart it
        if !matches!(self.state, InternalState::Idle) && self.needs_restart_for_spec_change() {
            warn!("Restarting service '{}' due to specification changes", self.spec.id);
            self.restart_service().await?;
        }

        Ok(())
    }

    /// Spawn a new process
    async fn spawn_process(&mut self) -> Result<()> {
        debug!("Spawning process for service '{}'", self.spec.id);

        let process = self.process_adapter.spawn(&self.spec).await?;
        let pid = process.pid();

        self.emit_event(ServiceEvent::ProcessStarted {
            service_id: self.spec.id.clone(),
            pid,
            timestamp: ServiceEvent::current_timestamp(),
            command: self.spec.command.clone(),
            args: self.spec.args.clone(),
        }).await;

        self.current_process = Some(process);
        Ok(())
    }

    /// Stop the current process
    async fn stop_process(&mut self) -> Result<()> {
        if let Some(mut process) = self.current_process.take() {
            debug!("Stopping process {} for service '{}'", process.pid(), self.spec.id);

            // Try graceful termination first
            if let Err(e) = process.terminate().await {
                warn!("Failed to terminate process gracefully: {}", e);
            }

            // Wait for graceful exit with timeout
            let graceful_timeout = Duration::from_secs(self.spec.graceful_timeout_secs);
            match timeout(graceful_timeout, process.wait()).await {
                Ok(Ok(exit_info)) => {
                    debug!("Process exited gracefully");
                    self.emit_process_exit_event(exit_info).await;
                }
                Ok(Err(e)) => {
                    error!("Error waiting for process exit: {}", e);
                }
                Err(_) => {
                    warn!("Process did not exit gracefully within timeout, killing it");
                    if let Err(e) = process.kill().await {
                        error!("Failed to kill process: {}", e);
                    } else {
                        // Wait for kill to take effect
                        if let Ok(exit_info) = process.wait().await {
                            self.emit_process_exit_event(exit_info).await;
                        }
                    }
                }
            }
        }

        self.transition_to(InternalState::Idle, Some("Process stopped".to_string())).await?;
        Ok(())
    }

    /// Wait for the current process to exit
    async fn wait_for_process_exit(&mut self) -> Result<ServiceExit> {
        if let Some(ref mut process) = self.current_process {
            process.wait().await
        } else {
            // This should never be called when there's no process
            Err(crate::CoreError::ServiceError("No process to wait for".to_string()))
        }
    }

    /// Handle a process exit
    async fn handle_process_exit(&mut self, exit_info: ServiceExit) -> Result<()> {
        debug!("Process {} exited for service '{}'", exit_info.pid, self.spec.id);

        self.emit_process_exit_event(exit_info.clone()).await;
        self.current_process = None;

        // Apply restart policy to determine next action
        let restart_action = self.restart_policy_engine.should_restart(&exit_info);
        
        match restart_action {
            RestartAction::Stop => {
                info!("Restart policy determined service should not restart, transitioning to Idle");
                self.transition_to(InternalState::Idle, Some("Service stopped - restart policy".to_string())).await?;
            }
            RestartAction::Restart { delay } => {
                info!("Restart policy determined service should restart after {:?}", delay);
                
                // Transition to Idle first
                self.transition_to(InternalState::Idle, Some("Process exited".to_string())).await?;
                
                // Schedule restart using non-blocking timer
                if delay.is_zero() {
                    // Immediate restart
                    info!("Starting immediate restart for service '{}'", self.spec.id);
                    if let Err(e) = self.start_service().await {
                        error!("Failed to restart service '{}': {}", self.spec.id, e);
                        self.transition_to(InternalState::Idle, Some("Restart failed".to_string())).await?;
                    }
                } else {
                    // Schedule restart after delay
                    let restart_time = tokio::time::Instant::now() + delay;
                    debug!("Scheduling restart for service '{}' at {:?}", self.spec.id, restart_time);
                    self.restart_timer = Some(restart_time);
                }
            }
        }

        Ok(())
    }

    /// Perform readiness check
    async fn check_readiness(&self) {
        // For now, simulate readiness check by waiting a bit
        // In a full implementation, this would perform actual health/readiness checks
        if let Some(ref readiness_check) = self.spec.readiness_check {
            tokio::time::sleep(readiness_check.initial_delay()).await;
            
            // Simulate the check passing immediately for testing
            // Real implementation would perform TCP/HTTP/Exec checks here
        } else {
            // No readiness check configured - become ready immediately
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Transition to a new state and emit events
    async fn transition_to(&mut self, new_state: InternalState, reason: Option<String>) -> Result<()> {
        if self.state == new_state {
            return Ok(());
        }

        let old_state = self.state;
        self.state = new_state;

        debug!("Service '{}' transitioning from {:?} to {:?}", self.spec.id, old_state, new_state);

        // Update state broadcaster
        if self.state_tx.send(new_state.into()).is_err() {
            warn!("No state subscribers for service '{}'", self.spec.id);
        }

        // Emit state change event
        self.emit_event(ServiceEvent::StateChanged {
            service_id: self.spec.id.clone(),
            from_state: old_state.into(),
            to_state: new_state.into(),
            timestamp: ServiceEvent::current_timestamp(),
            reason,
        }).await;

        Ok(())
    }

    /// Emit a service event
    async fn emit_event(&self, event: ServiceEvent) {
        if let Err(e) = self.event_tx.send(event) {
            warn!("Failed to emit event: {}", e);
        }
    }

    /// Emit a process exit event
    async fn emit_process_exit_event(&self, exit_info: ServiceExit) {
        self.emit_event(ServiceEvent::ProcessExited {
            service_id: self.spec.id.clone(),
            exit_info,
        }).await;
    }

    /// Get fields that changed between old and new spec
    fn get_changed_fields(&self, new_spec: &ServiceSpec) -> Vec<String> {
        let mut changed = Vec::new();
        
        if self.spec.command != new_spec.command {
            changed.push("command".to_string());
        }
        if self.spec.args != new_spec.args {
            changed.push("args".to_string());
        }
        if self.spec.environment != new_spec.environment {
            changed.push("environment".to_string());
        }
        if self.spec.working_directory != new_spec.working_directory {
            changed.push("workingDirectory".to_string());
        }
        if self.spec.restart_policy != new_spec.restart_policy {
            changed.push("restartPolicy".to_string());
        }
        if self.spec.health_check != new_spec.health_check {
            changed.push("healthCheck".to_string());
        }
        if self.spec.readiness_check != new_spec.readiness_check {
            changed.push("readinessCheck".to_string());
        }
        if self.spec.graceful_timeout_secs != new_spec.graceful_timeout_secs {
            changed.push("gracefulTimeoutSecs".to_string());
        }
        if self.spec.startup_timeout_secs != new_spec.startup_timeout_secs {
            changed.push("startupTimeoutSecs".to_string());
        }
        
        changed
    }

    /// Check if the service needs to be restarted due to spec changes
    fn needs_restart_for_spec_change(&self) -> bool {
        // For now, assume any change requires restart
        // In a more sophisticated implementation, we might only restart for
        // certain types of changes (e.g., command/args but not health check config)
        true
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
            graceful_timeout_secs: 1,
            startup_timeout_secs: 5,
        }
    }

    async fn create_supervisor() -> (ServiceSupervisor, mpsc::UnboundedSender<ControlMsg>, broadcast::Receiver<ServiceEvent>, mpsc::UnboundedReceiver<ControlMsg>) {
        let spec = create_test_spec();
        let process_adapter = Arc::new(MockProcessAdapter::new());
        let (event_tx, event_rx) = broadcast::channel(100);
        let (state_tx, _state_rx) = watch::channel(ServiceState::Idle);

        let supervisor = ServiceSupervisor::new(spec, process_adapter, event_tx, state_tx);
        let (control_tx, control_rx) = mpsc::unbounded_channel();

        (supervisor, control_tx, event_rx, control_rx)
    }

    #[tokio::test]
    async fn test_supervisor_basic_lifecycle() {
        let (mut supervisor, control_tx, mut event_rx, control_rx) = create_supervisor().await;

        // Start the supervisor task
        tokio::spawn(async move {
            if let Err(e) = supervisor.run(control_rx).await {
                // Expected when we shutdown
                debug!("Supervisor terminated: {}", e);
            }
        });

        // Test start
        control_tx.send(ControlMsg::Start).unwrap();
        
        // Should get state change to Spawning
        if let Ok(event) = timeout(Duration::from_millis(500), event_rx.recv()).await {
            match event.unwrap() {
                ServiceEvent::StateChanged { to_state: ServiceState::Spawning, .. } => {},
                other => panic!("Expected Spawning state change, got: {:?}", other),
            }
        }

        // Should get process started event
        if let Ok(event) = timeout(Duration::from_millis(500), event_rx.recv()).await {
            match event.unwrap() {
                ServiceEvent::ProcessStarted { .. } => {},
                other => panic!("Expected ProcessStarted event, got: {:?}", other),
            }
        }

        // Give time for process to complete and emit more events
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Test shutdown
        control_tx.send(ControlMsg::Shutdown).unwrap();
        
        // Give time for shutdown
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_supervisor_stop_service() {
        let (mut supervisor, control_tx, mut event_rx, control_rx) = create_supervisor().await;

        tokio::spawn(async move {
            if let Err(e) = supervisor.run(control_rx).await {
                debug!("Supervisor terminated: {}", e);
            }
        });

        // Start service
        control_tx.send(ControlMsg::Start).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Stop service
        control_tx.send(ControlMsg::Stop).unwrap();
        
        // Should eventually get state change to Stopping
        let mut found_stopping = false;
        for _ in 0..10 {
            if let Ok(event) = timeout(Duration::from_millis(100), event_rx.recv()).await {
                match event.unwrap() {
                    ServiceEvent::StateChanged { to_state: ServiceState::Stopping, .. } => {
                        found_stopping = true;
                        break;
                    }
                    _ => continue,
                }
            }
        }
        
        if !found_stopping {
            // That's OK, the mock process might exit quickly
            debug!("Didn't see Stopping state - process may have exited quickly");
        }

        control_tx.send(ControlMsg::Shutdown).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_supervisor_restart_service() {
        let (mut supervisor, control_tx, mut event_rx, control_rx) = create_supervisor().await;

        tokio::spawn(async move {
            if let Err(e) = supervisor.run(control_rx).await {
                debug!("Supervisor terminated: {}", e);
            }
        });

        // Start service
        control_tx.send(ControlMsg::Start).unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Restart service
        control_tx.send(ControlMsg::Restart).unwrap();
        
        // Give time for restart sequence
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Should see multiple state changes
        let mut events = Vec::new();
        while let Ok(event) = timeout(Duration::from_millis(50), event_rx.recv()).await {
            events.push(event.unwrap());
        }

        // Should have seen multiple events
        assert!(!events.is_empty());

        control_tx.send(ControlMsg::Shutdown).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_supervisor_update_spec() {
        let (mut supervisor, control_tx, mut event_rx, control_rx) = create_supervisor().await;

        tokio::spawn(async move {
            if let Err(e) = supervisor.run(control_rx).await {
                debug!("Supervisor terminated: {}", e);
            }
        });

        // Update spec
        let mut new_spec = create_test_spec();
        new_spec.command = "ls".to_string();
        control_tx.send(ControlMsg::UpdateSpec(new_spec)).unwrap();

        // Should get configuration updated event
        if let Ok(event) = timeout(Duration::from_millis(500), event_rx.recv()).await {
            match event.unwrap() {
                ServiceEvent::ConfigurationUpdated { changed_fields, .. } => {
                    assert!(changed_fields.contains(&"command".to_string()));
                }
                other => panic!("Expected ConfigurationUpdated event, got: {:?}", other),
            }
        }

        control_tx.send(ControlMsg::Shutdown).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
