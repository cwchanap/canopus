//! Process adapters for abstracting process management
//!
//! This module provides traits and implementations for abstracting process
//! management operations, enabling testing with mock implementations and
//! supporting different process management backends.

use crate::Result;
use async_trait::async_trait;
use schema::{ServiceExit, ServiceSpec};
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncRead;
use tracing::debug;

/// Trait for managing processes in a platform-agnostic way
#[async_trait]
pub trait ProcessAdapter: Send + Sync {
    /// Spawn a new managed process according to the service specification
    async fn spawn(&self, spec: &ServiceSpec) -> Result<Box<dyn ManagedProcess>>;
}

/// Trait representing a managed process that can be controlled and monitored
#[async_trait]
pub trait ManagedProcess: Send + Sync {
    /// Get the process ID
    fn pid(&self) -> u32;

    /// Wait for the process to exit
    async fn wait(&mut self) -> Result<ServiceExit>;

    /// Terminate the process gracefully (SIGTERM)
    async fn terminate(&mut self) -> Result<()>;

    /// Kill the process forcefully (SIGKILL)
    async fn kill(&mut self) -> Result<()>;

    /// Check if the process is still alive
    fn is_alive(&self) -> bool;

    /// Take a readable handle to the child's stdout for async consumption.
    /// This can be used to spawn tasks that read logs line-by-line.
    /// Returns None if stdout was not piped or already taken.
    fn take_stdout(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + Unpin>>>;

    /// Take a readable handle to the child's stderr for async consumption.
    /// Returns None if stderr was not piped or already taken.
    fn take_stderr(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + Unpin>>>;
}

/// Unix process adapter using the existing process management
#[cfg(unix)]
#[derive(Copy, Clone, Debug, Default)]
pub struct UnixProcessAdapter;

#[cfg(unix)]
impl UnixProcessAdapter {
    /// Create a new Unix process adapter
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[cfg(unix)]
#[async_trait]
impl ProcessAdapter for UnixProcessAdapter {
    async fn spawn(&self, spec: &ServiceSpec) -> Result<Box<dyn ManagedProcess>> {
        use crate::process::unix;

        debug!("Spawning Unix process: {} {:?}", spec.command, spec.args);

        // Build args vector
        let args: Vec<&str> = spec.args.iter().map(String::as_str).collect();

        // Spawn the process with environment and working directory
        let child = unix::spawn_with(
            &spec.command,
            &args,
            &spec.environment,
            spec.working_directory.as_deref(),
        )?;

        Ok(Box::new(UnixManagedProcess { child }))
    }
}

/// Unix managed process implementation
#[cfg(unix)]
struct UnixManagedProcess {
    child: crate::process::unix::ChildProcess,
}

#[cfg(unix)]
#[async_trait]
impl ManagedProcess for UnixManagedProcess {
    fn pid(&self) -> u32 {
        self.child.pid()
    }

    async fn wait(&mut self) -> Result<ServiceExit> {
        let exit_status = self.child.wait().await?;

        let (exit_code, signal) = exit_status.code().map_or_else(
            || {
                // On Unix, if there's no exit code, it was likely killed by a signal
                use std::os::unix::process::ExitStatusExt;
                let signal = exit_status.signal();
                (None, signal)
            },
            |code| (Some(code), None),
        );

        Ok(ServiceExit {
            pid: self.pid(),
            exit_code,
            signal,
            timestamp: schema::ServiceEvent::current_timestamp(),
        })
    }

    async fn terminate(&mut self) -> Result<()> {
        use crate::process::unix;
        unix::signal_term_group(&self.child)
    }

    async fn kill(&mut self) -> Result<()> {
        use crate::process::unix;
        unix::signal_kill_group(&self.child)
    }

    fn is_alive(&self) -> bool {
        // Use killpg(pgid, 0) to check if the process group still exists.
        // Signal 0 doesn't send any signal but checks if the process group
        // exists and we have permission to signal it. ESRCH means gone.
        // We use killpg instead of kill because:
        // 1. The child is spawned with setsid() making it the PG leader
        // 2. Process groups are recycled less frequently than individual PIDs
        // 3. This reduces (but doesn't eliminate) PID reuse false positives
        let pgid = self.child.pgid();
        // SAFETY: killpg with signal 0 is a standard POSIX probe for process group existence.
        #[allow(unsafe_code)]
        let ret = match i32::try_from(pgid) {
            Ok(pgid_i32) => unsafe { libc::killpg(pgid_i32, 0) },
            Err(_) => return false, // PGID too large, consider process not alive
        };
        if ret == 0 {
            return true;
        }

        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EPERM) => true,
            Some(libc::ESRCH) => false,
            _ => false,
        }
    }

    fn take_stdout(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + Unpin>>> {
        self.child.take_stdout().map(|s| {
            let r: Pin<Box<dyn AsyncRead + Send + Unpin>> = Box::pin(s);
            r
        })
    }

    fn take_stderr(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + Unpin>>> {
        self.child.take_stderr().map(|s| {
            let r: Pin<Box<dyn AsyncRead + Send + Unpin>> = Box::pin(s);
            r
        })
    }
}

/// Mock process adapter for testing
#[derive(Debug, Clone)]
pub struct MockProcessAdapter {
    /// Instructions for mock processes
    instructions: Arc<tokio::sync::Mutex<Vec<MockInstruction>>>,
}

/// Instructions for mock process behavior
#[derive(Debug, Clone, Copy)]
pub struct MockInstruction {
    /// How long to wait before the process "exits"
    pub exit_delay: std::time::Duration,
    /// Exit code to return (None means killed by signal)
    pub exit_code: Option<i32>,
    /// Signal that killed the process (Unix only)
    pub signal: Option<i32>,
    /// Whether terminate/kill commands should work immediately
    pub responds_to_signals: bool,
}

impl Default for MockInstruction {
    fn default() -> Self {
        Self {
            exit_delay: std::time::Duration::from_millis(100),
            exit_code: Some(0),
            signal: None,
            responds_to_signals: true,
        }
    }
}

impl MockProcessAdapter {
    /// Create a new mock adapter with no pre-configured instructions
    #[must_use]
    pub fn new() -> Self {
        Self {
            instructions: Arc::new(tokio::sync::Mutex::new(vec![])),
        }
    }

    /// Add instructions for the next spawned process
    pub async fn add_instruction(&self, instruction: MockInstruction) {
        let mut instructions = self.instructions.lock().await;
        instructions.push(instruction);
    }

    /// Set instructions for all future spawned processes
    pub async fn set_instructions(&self, instructions: Vec<MockInstruction>) {
        let mut current = self.instructions.lock().await;
        *current = instructions;
    }

    /// Create a mock that always succeeds quickly
    #[must_use]
    pub fn success() -> Self {
        let adapter = Self::new();
        let adapter_clone = adapter.clone();
        tokio::spawn(async move {
            adapter_clone
                .set_instructions(vec![MockInstruction {
                    exit_delay: std::time::Duration::from_millis(50),
                    exit_code: Some(0),
                    signal: None,
                    responds_to_signals: true,
                }])
                .await;
        });
        adapter
    }

    /// Create a mock that always fails
    #[must_use]
    pub fn failure() -> Self {
        let adapter = Self::new();
        let adapter_clone = adapter.clone();
        tokio::spawn(async move {
            adapter_clone
                .set_instructions(vec![MockInstruction {
                    exit_delay: std::time::Duration::from_millis(50),
                    exit_code: Some(1),
                    signal: None,
                    responds_to_signals: true,
                }])
                .await;
        });
        adapter
    }

    /// Create a mock that takes a long time to start
    #[must_use]
    pub fn slow_start() -> Self {
        let adapter = Self::new();
        let adapter_clone = adapter.clone();
        tokio::spawn(async move {
            adapter_clone
                .set_instructions(vec![MockInstruction {
                    exit_delay: std::time::Duration::from_secs(5),
                    exit_code: Some(0),
                    signal: None,
                    responds_to_signals: true,
                }])
                .await;
        });
        adapter
    }
}

impl Default for MockProcessAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProcessAdapter for MockProcessAdapter {
    async fn spawn(&self, spec: &ServiceSpec) -> Result<Box<dyn ManagedProcess>> {
        debug!(
            "Spawning mock process for: {} {:?}",
            spec.command, spec.args
        );

        let mut instructions = self.instructions.lock().await;
        let instruction = if instructions.is_empty() {
            MockInstruction::default()
        } else {
            instructions.remove(0)
        };

        // Generate a fake PID
        let pid = rand::random::<u32>() % 65536 + 1000;

        Ok(Box::new(MockManagedProcess::new(pid, instruction)))
    }
}

/// Mock managed process for testing
struct MockManagedProcess {
    pid: u32,
    instruction: MockInstruction,
    started_at: std::time::Instant,
    terminated: bool,
    killed: bool,
}

impl MockManagedProcess {
    fn new(pid: u32, instruction: MockInstruction) -> Self {
        Self {
            pid,
            instruction,
            started_at: std::time::Instant::now(),
            terminated: false,
            killed: false,
        }
    }

    fn should_exit(&self) -> bool {
        if self.killed || self.terminated {
            return true;
        }

        self.started_at.elapsed() >= self.instruction.exit_delay
    }

    fn create_exit(&self) -> ServiceExit {
        let (exit_code, signal) = if self.killed && self.instruction.responds_to_signals {
            (None, Some(9)) // SIGKILL
        } else if self.terminated && self.instruction.responds_to_signals {
            (None, Some(15)) // SIGTERM
        } else {
            (self.instruction.exit_code, self.instruction.signal)
        };

        ServiceExit {
            pid: self.pid,
            exit_code,
            signal,
            timestamp: schema::ServiceEvent::current_timestamp(),
        }
    }
}

#[async_trait]
impl ManagedProcess for MockManagedProcess {
    fn pid(&self) -> u32 {
        self.pid
    }

    async fn wait(&mut self) -> Result<ServiceExit> {
        // Simulate waiting for the process to exit
        while !self.should_exit() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        Ok(self.create_exit())
    }

    async fn terminate(&mut self) -> Result<()> {
        debug!("Terminating mock process {}", self.pid);
        self.terminated = true;
        Ok(())
    }

    async fn kill(&mut self) -> Result<()> {
        debug!("Killing mock process {}", self.pid);
        self.killed = true;
        Ok(())
    }

    fn is_alive(&self) -> bool {
        !self.should_exit()
    }

    fn take_stdout(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + Unpin>>> {
        // For now, mock process does not produce stdout. This can be enhanced to
        // return a synthetic AsyncRead for testing log capture.
        None
    }

    fn take_stderr(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + Unpin>>> {
        None
    }
}

// Simple random number generator for mock PIDs using the shared LCG
mod rand {
    pub(super) fn random<T>() -> T
    where
        T: From<u32>,
    {
        T::from(crate::utilities::simple_rng::next_u32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schema::BackoffConfig;
    use std::collections::HashMap;
    use std::time::Duration;

    fn create_test_spec() -> ServiceSpec {
        ServiceSpec {
            id: "test".to_string(),
            name: "Test".to_string(),
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            environment: HashMap::default(),
            working_directory: None,
            route: None,
            restart_policy: schema::RestartPolicy::Never,
            backoff_config: BackoffConfig::default(),
            health_check: None,
            readiness_check: None,
            graceful_timeout_secs: 5,
            startup_timeout_secs: 10,
        }
    }

    #[tokio::test]
    async fn test_mock_adapter_spawn() {
        let adapter = MockProcessAdapter::new();
        let spec = create_test_spec();

        let process = adapter.spawn(&spec).await.unwrap();
        assert!(process.pid() > 0);
        assert!(process.is_alive());
    }

    #[tokio::test]
    async fn test_mock_process_wait() {
        let adapter = MockProcessAdapter::new();
        let spec = create_test_spec();

        let mut process = adapter.spawn(&spec).await.unwrap();
        let exit = process.wait().await.unwrap();

        assert_eq!(exit.exit_code, Some(0));
        assert_eq!(exit.signal, None);
        assert_eq!(exit.pid, process.pid());
    }

    #[tokio::test]
    async fn test_mock_process_terminate() {
        let adapter = MockProcessAdapter::new();
        adapter
            .add_instruction(MockInstruction {
                exit_delay: Duration::from_secs(10), // Long delay
                exit_code: Some(0),
                signal: None,
                responds_to_signals: true,
            })
            .await;

        let spec = create_test_spec();
        let mut process = adapter.spawn(&spec).await.unwrap();

        // Process should be alive initially
        assert!(process.is_alive());

        // Terminate it
        process.terminate().await.unwrap();

        // Wait should return quickly with signal
        let exit = process.wait().await.unwrap();
        assert_eq!(exit.exit_code, None);
        assert_eq!(exit.signal, Some(15)); // SIGTERM
    }

    #[tokio::test]
    async fn test_mock_process_kill() {
        let adapter = MockProcessAdapter::new();
        adapter
            .add_instruction(MockInstruction {
                exit_delay: Duration::from_secs(10), // Long delay
                exit_code: Some(0),
                signal: None,
                responds_to_signals: true,
            })
            .await;

        let spec = create_test_spec();
        let mut process = adapter.spawn(&spec).await.unwrap();

        // Kill it
        process.kill().await.unwrap();

        // Wait should return quickly with SIGKILL
        let exit = process.wait().await.unwrap();
        assert_eq!(exit.exit_code, None);
        assert_eq!(exit.signal, Some(9)); // SIGKILL
    }

    #[tokio::test]
    async fn test_mock_adapter_factory_methods() {
        // Test success factory
        let adapter = MockProcessAdapter::success();
        let spec = create_test_spec();

        // Wait a bit for the async instruction setup
        tokio::time::sleep(Duration::from_millis(10)).await;

        let mut process = adapter.spawn(&spec).await.unwrap();
        let exit = process.wait().await.unwrap();
        assert_eq!(exit.exit_code, Some(0));

        // Test failure factory
        let adapter = MockProcessAdapter::failure();
        tokio::time::sleep(Duration::from_millis(10)).await;

        let mut process = adapter.spawn(&spec).await.unwrap();
        let exit = process.wait().await.unwrap();
        assert_eq!(exit.exit_code, Some(1));
    }
}
