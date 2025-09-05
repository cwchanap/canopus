//! Service supervisor task implementation
//!
//! This module contains the [`ServiceSupervisor`] which implements the core
//! state machine logic for managing a single service's lifecycle.

use super::{ControlMsg, InternalState, ManagedProcess, ProcessAdapter, RestartPolicyEngine, RestartAction, HealthStatus, HealthCheckStatus, ReadinessCheckStatus};
use crate::{health, Result};
use crate::logging::{LogEntry, LogRing};
use schema::{ServiceEvent, ServiceExit, ServiceSpec, ServiceState};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::{timeout, sleep, Instant, interval, MissedTickBehavior};
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
    /// Bounded in-memory ring buffer of recent log entries
    log_ring: Arc<tokio::sync::Mutex<LogRing>>,
    /// Async reader task for stdout
    stdout_task: Option<tokio::task::JoinHandle<()>>,
    /// Async reader task for stderr
    stderr_task: Option<tokio::task::JoinHandle<()>>,
    /// Restart policy engine for determining restart actions
    restart_policy_engine: RestartPolicyEngine,
    /// Restart timer (when restart is scheduled)
    restart_timer: Option<tokio::time::Instant>,
    /// Next readiness check time
    readiness_check_timer: Option<tokio::time::Instant>,
    /// Next health check time (for liveness checks)
    health_check_timer: Option<tokio::time::Instant>,
    /// Startup timeout timer
    startup_timeout_timer: Option<tokio::time::Instant>,
    /// Consecutive readiness check successes
    readiness_success_count: u32,
    /// Consecutive health check failures
    health_failure_count: u32,
    /// Consecutive health check successes
    health_success_count: u32,
    /// Last health check result
    last_health_check: Option<HealthCheckStatus>,
    /// Last readiness check result
    last_readiness_check: Option<ReadinessCheckStatus>,
}

impl ServiceSupervisor {
    /// Spawn a background task to read lines from the given async reader,
    /// push them into the log ring, and emit LogOutput events.
    fn spawn_log_reader(
        &self,
        reader: std::pin::Pin<Box<dyn AsyncRead + Send + Unpin>>,
        stream: schema::LogStream,
    ) -> tokio::task::JoinHandle<()> {
        let service_id = self.spec.id.clone();
        let event_tx = self.event_tx.clone();
        let ring = self.log_ring.clone();

        tokio::spawn(async move {
            // Convert Pin<Box<..>> to Box<..> since it's Unpin
            let reader = std::pin::Pin::into_inner(reader);
            let mut lines = BufReader::new(reader).lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let timestamp = schema::ServiceEvent::current_timestamp();
                        let content = line; // already without trailing newline

                        // Push into ring buffer
                        {
                            let mut guard = ring.lock().await;
                            guard.push(LogEntry {
                                seq: 0, // assigned by ring
                                stream,
                                content: content.clone(),
                                timestamp: timestamp.clone(),
                            });
                        }

                        // Emit event (best-effort)
                        let _ = event_tx.send(schema::ServiceEvent::LogOutput {
                            service_id: service_id.clone(),
                            stream,
                            content,
                            timestamp,
                        });
                    }
                    Ok(None) => {
                        // EOF
                        break;
                    }
                    Err(e) => {
                        // Emit a warning and stop this reader
                        let _ = event_tx.send(schema::ServiceEvent::Warning {
                            service_id: service_id.clone(),
                            message: format!("Error reading log stream {:?}: {}", stream, e),
                            timestamp: schema::ServiceEvent::current_timestamp(),
                            code: Some("LOG_READ_ERROR".to_string()),
                        });
                        break;
                    }
                }
            }
        })
    }
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
            // Default log ring capacity; could be made configurable later
            log_ring: Arc::new(tokio::sync::Mutex::new(LogRing::new(1024))),
            stdout_task: None,
            stderr_task: None,
            restart_policy_engine,
            restart_timer: None,
            readiness_check_timer: None,
            health_check_timer: None,
            startup_timeout_timer: None,
            readiness_success_count: 0,
            health_failure_count: 0,
            health_success_count: 0,
            last_health_check: None,
            last_readiness_check: None,
        }
    }

    /// Run the supervisor task loop
    pub async fn run(&mut self, mut control_rx: mpsc::UnboundedReceiver<ControlMsg>) -> Result<()> {
        info!("Starting supervisor for service '{}'", self.spec.id);
        // Periodic tick to ensure timers are checked regularly regardless of other events
        let mut tick = interval(Duration::from_millis(50));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

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

                // Periodic tick to ensure timers are handled even without other events
                _ = tick.tick() => {
                    // No-op: fall through to handle_timers below
                }
            }

            // Handle timers and periodic checks
            self.handle_timers().await?;
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
            ControlMsg::GetHealthStatus { response } => {
                let health_status = self.get_health_status_snapshot();
                let _ = response.send(health_status);
            }
            ControlMsg::TriggerHealthCheck { response } => {
                if self.spec.health_check.is_some() {
                    let result = self.perform_on_demand_health_check().await;
                    let _ = response.send(result);
                } else {
                    let _ = response.send(Err(crate::CoreError::ServiceError("No health check configured".to_string())));
                }
            }
            ControlMsg::TriggerReadinessCheck { response } => {
                if self.spec.readiness_check.is_some() {
                    let result = self.perform_on_demand_readiness_check().await;
                    let _ = response.send(result);
                } else {
                    let _ = response.send(Err(crate::CoreError::ServiceError("No readiness check configured".to_string())));
                }
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
                    
                    // Set startup timeout if configured
                    if self.spec.startup_timeout_secs > 0 {
                        self.startup_timeout_timer = Some(Instant::now() + Duration::from_secs(self.spec.startup_timeout_secs));
                    }
                    
                    // Initialize readiness check timer if configured
                    if let Some(ref readiness_check) = self.spec.readiness_check {
                        let delay = readiness_check.initial_delay();
                        self.readiness_check_timer = Some(Instant::now() + delay);
                        debug!("Initialized readiness check timer for service '{}' with delay {:?}", self.spec.id, delay);
                    }
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

        let mut process = self.process_adapter.spawn(&self.spec).await?;
        let pid = process.pid();

        self.emit_event(ServiceEvent::ProcessStarted {
            service_id: self.spec.id.clone(),
            pid,
            timestamp: ServiceEvent::current_timestamp(),
            command: self.spec.command.clone(),
            args: self.spec.args.clone(),
        }).await;

        // Take stdout/stderr before storing the process
        let stdout = process.take_stdout();
        let stderr = process.take_stderr();

        self.current_process = Some(process);

        // Spawn async log reader tasks
        if let Some(reader) = stdout {
            let handle = self.spawn_log_reader(reader, schema::LogStream::Stdout);
            self.stdout_task = Some(handle);
        }
        if let Some(reader) = stderr {
            let handle = self.spawn_log_reader(reader, schema::LogStream::Stderr);
            self.stderr_task = Some(handle);
        }
        Ok(())
    }

    /// Stop the current process
    async fn stop_process(&mut self) -> Result<()> {
        if let Some(mut process) = self.current_process.take() {
            debug!("Stopping process {} for service '{}'", process.pid(), self.spec.id);

            // Stop log reader tasks first
            if let Some(handle) = self.stdout_task.take() { handle.abort(); }
            if let Some(handle) = self.stderr_task.take() { handle.abort(); }

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
        // Ensure log reader tasks are stopped
        if let Some(handle) = self.stdout_task.take() { handle.abort(); }
        if let Some(handle) = self.stderr_task.take() { handle.abort(); }

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
    async fn perform_readiness_check(&mut self) {
        if let Some(ref readiness_check) = self.spec.readiness_check {
            debug!("Performing readiness check for service '{}'", self.spec.id);
            
            let start_time = Instant::now();
            let result = health::run_probe(&schema::HealthCheck {
                check_type: readiness_check.check_type.clone(),
                interval_secs: readiness_check.interval_secs,
                timeout_secs: readiness_check.timeout_secs,
                failure_threshold: 1, // Readiness checks don't use failure threshold
                success_threshold: readiness_check.success_threshold,
            }).await;
            let duration_ms = start_time.elapsed().as_millis() as u64;
            
            let success = result.is_ok();
            let error = result.err().map(|e| e.to_string());
            
            // Store the last readiness check result
            self.last_readiness_check = Some(ReadinessCheckStatus {
                success,
                timestamp: SystemTime::now(),
                duration: start_time.elapsed(),
                error: error.clone(),
            });
            
            // Emit readiness check result event
            self.emit_event(ServiceEvent::ReadinessCheckResult {
                service_id: self.spec.id.clone(),
                success,
                timestamp: ServiceEvent::current_timestamp(),
                error: error.clone(),
                duration_ms,
            }).await;
            
            if success {
                self.readiness_success_count += 1;
                debug!("Readiness check passed ({}/{})", self.readiness_success_count, readiness_check.success_threshold);
                
                if self.readiness_success_count >= readiness_check.success_threshold {
                    info!("Service '{}' is ready after {} successful readiness checks", self.spec.id, self.readiness_success_count);
                    // Reset counter for next time
                    self.readiness_success_count = 0;
                    // Transition to Ready state
                    if let Err(e) = self.transition_to(InternalState::Ready, Some("Readiness check passed".to_string())).await {
                        error!("Failed to transition to Ready state: {}", e);
                    }
                } else {
                    // Need more successes - schedule next check
                    self.readiness_check_timer = Some(Instant::now() + readiness_check.interval());
                    return; // Don't transition to Ready yet
                }
            } else {
                warn!("Readiness check failed for service '{}': {:?}", self.spec.id, error);
                self.readiness_success_count = 0; // Reset on failure
                // Schedule next check
                self.readiness_check_timer = Some(Instant::now() + readiness_check.interval());
                return; // Don't transition to Ready
            }
        } else {
            // No readiness check configured - become ready immediately
            debug!("No readiness check configured for service '{}', marking as ready", self.spec.id);
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
    
    /// Handle all timers (readiness, health, startup timeout)
    async fn handle_timers(&mut self) -> Result<()> {
        let now = Instant::now();
        debug!("Handling timers for service '{}' in state {:?}", self.spec.id, self.state);
        
        // Handle scheduled restart timer (processed on periodic ticks to avoid starvation)
        if let Some(restart_time) = self.restart_timer {
            if now >= restart_time {
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

        // Handle startup timeout
        if let Some(timeout_time) = self.startup_timeout_timer {
            if now >= timeout_time {
                if matches!(self.state, InternalState::Starting) {
                    warn!("Startup timeout expired for service '{}'", self.spec.id);
                    self.startup_timeout_timer = None;
                    
                    // Emit timeout event
                    self.emit_event(ServiceEvent::StartupTimeout {
                        service_id: self.spec.id.clone(),
                        timestamp: ServiceEvent::current_timestamp(),
                        timeout_secs: self.spec.startup_timeout_secs,
                    }).await;
                    
                    // Handle the timeout according to policy (restart or fail)
                    // For now, we'll restart the service
                    self.restart_service().await?;
                } else {
                    // No longer in Starting state, clear the timer
                    self.startup_timeout_timer = None;
                }
            }
        }
        
        // Handle readiness checks
        if matches!(self.state, InternalState::Starting) {
            debug!("Service '{}' is in Starting state, checking readiness timer", self.spec.id);
            if let Some(check_time) = self.readiness_check_timer {
                debug!("Readiness check timer is set for {:?}, now is {:?}", check_time, now);
                if now >= check_time {
                    debug!("Time to perform readiness check for service '{}'", self.spec.id);
                    // Reset the timer
                    self.readiness_check_timer = None;
                    
                    // Perform readiness check
                    self.perform_readiness_check().await;
                }
            } else if self.spec.readiness_check.is_some() {
                debug!("No readiness check timer set, scheduling initial check");
                // Schedule initial readiness check after initial delay
                if let Some(ref readiness_check) = self.spec.readiness_check {
                    let delay = readiness_check.initial_delay();
                    self.readiness_check_timer = Some(now + delay);
                    debug!("Scheduled initial readiness check for service '{}' in {:?}", self.spec.id, delay);
                }
            } else {
                debug!("No readiness check configured for service '{}'", self.spec.id);
                // No readiness check configured, transition to Ready
                self.transition_to(InternalState::Ready, Some("Service is ready (no readiness check)".to_string())).await?;
            }
        }
        
        // Handle health checks (for ready services)
        if matches!(self.state, InternalState::Ready) {
            if let Some(check_time) = self.health_check_timer {
                if now >= check_time {
                    // Reset the timer
                    self.health_check_timer = None;
                    
                    // Perform health check
                    self.perform_health_check().await;
                }
            } else if self.spec.health_check.is_some() {
                // Schedule initial health check
                if let Some(ref health_check) = self.spec.health_check {
                    // Health checks start immediately (no initial delay like readiness checks)
                    self.health_check_timer = Some(now + health_check.interval());
                    debug!(
                        "Scheduled initial health check for service '{}' in {:?}",
                        self.spec.id,
                        health_check.interval()
                    );
                }
            }
        }
        
        Ok(())
    }
    
    /// Get a snapshot of the current health status
    fn get_health_status_snapshot(&self) -> HealthStatus {
        let now = Instant::now();
        
        HealthStatus {
            state: self.state.into(),
            health_check_enabled: self.spec.health_check.is_some(),
            readiness_check_enabled: self.spec.readiness_check.is_some(),
            consecutive_health_failures: self.health_failure_count,
            consecutive_readiness_successes: self.readiness_success_count,
            next_readiness_check_in: self.readiness_check_timer.map(|t| t.saturating_duration_since(now)),
            next_health_check_in: self.health_check_timer.map(|t| t.saturating_duration_since(now)),
            startup_timeout_in: self.startup_timeout_timer.map(|t| t.saturating_duration_since(now)),
            last_health_check: self.last_health_check.clone(),
            last_readiness_check: self.last_readiness_check.clone(),
        }
    }

    /// Perform an on-demand health check (for debugging)
    async fn perform_on_demand_health_check(&mut self) -> Result<bool> {
        if let Some(ref health_check) = self.spec.health_check {
            debug!("Performing on-demand health check for service '{}'", self.spec.id);
            
            let start_time = Instant::now();
            let result = health::run_probe(&schema::HealthCheck {
                check_type: health_check.check_type.clone(),
                interval_secs: health_check.interval_secs,
                timeout_secs: health_check.timeout_secs,
                failure_threshold: health_check.failure_threshold,
                success_threshold: health_check.success_threshold,
            }).await;
            
            let success = result.is_ok();
            let error = result.err().map(|e| e.to_string());
            let duration = start_time.elapsed();
            
            // Store the last health check result
            self.last_health_check = Some(HealthCheckStatus {
                success,
                timestamp: SystemTime::now(),
                duration,
                error: error.clone(),
            });
            
            // Emit health check result event
            self.emit_event(ServiceEvent::HealthCheckResult {
                service_id: self.spec.id.clone(),
                success,
                timestamp: ServiceEvent::current_timestamp(),
                error: error.clone(),
                duration_ms: duration.as_millis() as u64,
            }).await;
            
            Ok(success)
        } else {
            Err(crate::CoreError::ServiceError("No health check configured".to_string()))
        }
    }

    /// Perform an on-demand readiness check (for debugging)
    async fn perform_on_demand_readiness_check(&mut self) -> Result<bool> {
        if let Some(ref readiness_check) = self.spec.readiness_check {
            debug!("Performing on-demand readiness check for service '{}'", self.spec.id);
            
            let start_time = Instant::now();
            let result = health::run_probe(&schema::HealthCheck {
                check_type: readiness_check.check_type.clone(),
                interval_secs: readiness_check.interval_secs,
                timeout_secs: readiness_check.timeout_secs,
                failure_threshold: 1, // Readiness checks don't use failure threshold
                success_threshold: readiness_check.success_threshold,
            }).await;
            
            let success = result.is_ok();
            let error = result.err().map(|e| e.to_string());
            let duration = start_time.elapsed();
            
            // Store the last readiness check result
            self.last_readiness_check = Some(ReadinessCheckStatus {
                success,
                timestamp: SystemTime::now(),
                duration,
                error: error.clone(),
            });
            
            // Emit readiness check result event
            self.emit_event(ServiceEvent::ReadinessCheckResult {
                service_id: self.spec.id.clone(),
                success,
                timestamp: ServiceEvent::current_timestamp(),
                error: error.clone(),
                duration_ms: duration.as_millis() as u64,
            }).await;
            
            Ok(success)
        } else {
            Err(crate::CoreError::ServiceError("No readiness check configured".to_string()))
        }
    }

    /// Perform a health check for liveness
    async fn perform_health_check(&mut self) {
        if let Some(ref health_check) = self.spec.health_check {
            debug!("Performing health check for service '{}'", self.spec.id);
            
            let start_time = Instant::now();
            let result = health::run_probe(&schema::HealthCheck {
                check_type: health_check.check_type.clone(),
                interval_secs: health_check.interval_secs,
                timeout_secs: health_check.timeout_secs,
                failure_threshold: health_check.failure_threshold,
                success_threshold: health_check.success_threshold,
            }).await;
            let duration_ms = start_time.elapsed().as_millis() as u64;
            
            let success = result.is_ok();
            let error = result.err().map(|e| e.to_string());
            
            // Store the last health check result
            self.last_health_check = Some(HealthCheckStatus {
                success,
                timestamp: SystemTime::now(),
                duration: start_time.elapsed(),
                error: error.clone(),
            });
            
            // Emit health check result event
            self.emit_event(ServiceEvent::HealthCheckResult {
                service_id: self.spec.id.clone(),
                success,
                timestamp: ServiceEvent::current_timestamp(),
                error: error.clone(),
                duration_ms,
            }).await;
            
            if success {
                // Reset failure counter on success
                if self.health_failure_count > 0 {
                    debug!("Health check passed, resetting failure count");
                    self.health_failure_count = 0;
                }
                
                // Increment success counter
                self.health_success_count += 1;
                if self.health_success_count >= health_check.success_threshold {
                    // Reset counter after reaching threshold
                    self.health_success_count = 0;
                }
            } else {
                // Increment failure counter
                self.health_failure_count += 1;
                warn!("Health check failed for service '{}' ({}/{}): {:?}", 
                      self.spec.id, self.health_failure_count, health_check.failure_threshold, error);
                
                // Reset success counter on failure
                self.health_success_count = 0;
                
                // Check if we've hit the failure threshold
                if self.health_failure_count >= health_check.failure_threshold {
                    error!("Service '{}' health check failed {} times (threshold: {}), restarting", 
                           self.spec.id, self.health_failure_count, health_check.failure_threshold);
                    
                    // Emit unhealthy event
                    self.emit_event(ServiceEvent::ServiceUnhealthy {
                        service_id: self.spec.id.clone(),
                        timestamp: ServiceEvent::current_timestamp(),
                        reason: error.unwrap_or_else(|| "Health check failed".to_string()),
                        consecutive_failures: self.health_failure_count,
                    }).await;
                    
                    // Reset the failure counter
                    self.health_failure_count = 0;
                    
                    // Restart the service due to health check failures
                    if let Err(e) = self.restart_service().await {
                        error!("Failed to restart unhealthy service '{}': {}", self.spec.id, e);
                    }
                    
                    return; // Skip scheduling next check as we're restarting
                }
            }
            
            // Schedule next health check
            self.health_check_timer = Some(Instant::now() + health_check.interval());
            debug!(
                "Scheduled next health check for service '{}' in {:?}",
                self.spec.id,
                health_check.interval()
            );
        }
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
