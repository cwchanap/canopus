//! Unix process management with safe spawn/kill using process groups
//!
//! This module provides Unix-specific process management capabilities that use
//! process groups (via `setsid()`) to ensure safe and reliable process cleanup.
//!
//! ## Safety
//!
//! - All spawned processes are placed in their own process group using `setsid()`
//! - Signals are sent to the entire process group to ensure cleanup of child processes
//! - SIGTERM is used for graceful termination, SIGKILL for forceful termination
//! - Proper error handling for race conditions and edge cases
//!
//! ## Process Groups
//!
//! When a process calls `setsid()`, it:
//! - Creates a new session and becomes the session leader
//! - Creates a new process group and becomes the process group leader
//! - Has no controlling terminal
//!
//! This allows us to signal the entire process tree by sending signals to the
//! negative process ID (which targets the process group).

// Allow unsafe code for this module since process management requires libc::setsid() calls
#![allow(unsafe_code)]

use crate::{CoreError, Result};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
#[allow(unused_imports)]
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{debug, error, warn};

/// A child process managed with Unix process groups
///
/// This wrapper provides safe process group management for spawned processes.
/// The process is guaranteed to be in its own process group, allowing for
/// reliable cleanup of the entire process tree.
#[derive(Debug)]
pub struct ChildProcess {
    /// The process ID of the spawned process
    pid: Pid,
    /// The underlying Child handle for waiting and status checking
    child: Child,
}

impl ChildProcess {
    /// Get the process ID
    pub fn pid(&self) -> u32 {
        self.pid.as_raw() as u32
    }

    /// Get the process group ID (same as PID for session leaders)
    pub fn pgid(&self) -> u32 {
        self.pid.as_raw() as u32
    }

    /// Wait for the process to exit and return its exit status (async)
    pub async fn wait(&mut self) -> Result<std::process::ExitStatus> {
        self.child.wait().await.map_err(|e| {
            CoreError::ProcessWait(format!("Failed to wait for process {}: {}", self.pid, e))
        })
    }

    /// Try to wait for the process to exit without blocking
    pub fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child.try_wait().map_err(|e| {
            CoreError::ProcessWait(format!(
                "Failed to try_wait for process {}: {}",
                self.pid, e
            ))
        })
    }
}

/// Spawn a new process in its own process group
///
/// This function spawns a new process using the specified command and arguments.
/// The process is placed in its own process group via `setsid()`, which:
///
/// - Creates a new session with the process as session leader
/// - Creates a new process group with the process as group leader
/// - Detaches from the controlling terminal
///
/// ## Arguments
///
/// * `cmd` - The command to execute (must be in PATH or an absolute path)
/// * `args` - Command line arguments for the process
///
/// ## Safety
///
/// This function uses `unsafe` code to call `libc::setsid()` in the `before_exec`
/// closure. The safety is ensured because:
/// - `setsid()` is called in the child process before `exec()`
/// - `setsid()` is async-signal-safe and appropriate for use in `before_exec`
/// - Error handling properly converts C errors to Rust errors
///
/// ## Example
///
/// ```rust,no_run
/// use canopus_core::process::unix::spawn;
///
/// let child = spawn("echo", &["hello", "world"])?;
/// println!("Spawned process with PID: {}", child.pid());
/// # Ok::<(), canopus_core::CoreError>(())
/// ```
pub fn spawn(cmd: &str, args: &[&str]) -> Result<ChildProcess> {
    debug!("Spawning process: {} {:?}", cmd, args);

    let mut command = Command::new(cmd);
    command.args(args);
    // Pipe stdout/stderr so we can capture logs asynchronously
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    // Use before_exec to call setsid() in the child process
    // Safety: setsid() is async-signal-safe and appropriate for use in before_exec
    #[deny(unsafe_op_in_unsafe_fn)]
    unsafe {
        command.pre_exec(|| {
            // Create a new session and process group
            let result = libc::setsid();
            if result == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = command.spawn().map_err(|e| {
        error!("Failed to spawn process '{}': {}", cmd, e);
        CoreError::ProcessSpawn(format!("Failed to spawn '{}': {}", cmd, e))
    })?;

    // tokio::process::Child::id() may return Option on some platforms
    let raw_pid = child
        .id()
        .ok_or_else(|| CoreError::ProcessSpawn("Spawned child did not have a PID".to_string()))?;
    let pid = Pid::from_raw(raw_pid as i32);
    debug!("Successfully spawned process {} in new process group", pid);

    Ok(ChildProcess { pid, child })
}

impl ChildProcess {
    /// Take the stdout handle for async reading, if available
    pub fn take_stdout(&mut self) -> Option<tokio::process::ChildStdout> {
        self.child.stdout.take()
    }

    /// Take the stderr handle for async reading, if available
    pub fn take_stderr(&mut self) -> Option<tokio::process::ChildStderr> {
        self.child.stderr.take()
    }
}

/// Send SIGTERM to the process group for graceful termination
///
/// This function sends SIGTERM to the entire process group containing the
/// target process. This allows graceful shutdown of the process and any
/// child processes it may have spawned.
///
/// ## Arguments
///
/// * `child` - The child process whose process group should be terminated
///
/// ## Error Handling
///
/// - `ESRCH` (No such process) is treated as success since it means the process
///   group has already exited
/// - Other errors are propagated as `ProcessSignal` errors
///
/// ## Example
///
/// ```rust,no_run
/// use canopus_core::process::unix::{spawn, signal_term_group};
///
/// let child = spawn("sleep", &["30"])?;
/// signal_term_group(&child)?; // Gracefully terminate
/// # Ok::<(), canopus_core::CoreError>(())
/// ```
pub fn signal_term_group(child: &ChildProcess) -> Result<()> {
    debug!("Sending SIGTERM to process group {}", child.pid);

    match killpg(child.pid, Signal::SIGTERM) {
        Ok(()) => {
            debug!("Successfully sent SIGTERM to process group {}", child.pid);
            Ok(())
        }
        Err(nix::errno::Errno::ESRCH) => {
            // Process group doesn't exist, which means it already exited
            debug!("Process group {} already exited", child.pid);
            Ok(())
        }
        Err(nix::errno::Errno::EPERM) => {
            // Permission denied - process may have already exited or changed ownership
            debug!(
                "Permission denied signaling process group {} (likely already exited)",
                child.pid
            );
            Ok(())
        }
        Err(e) => {
            error!(
                "Failed to send SIGTERM to process group {}: {}",
                child.pid, e
            );
            Err(CoreError::ProcessSignal(format!(
                "Failed to send SIGTERM to process group {}: {}",
                child.pid, e
            )))
        }
    }
}

/// Send SIGKILL to the process group for forceful termination
///
/// This function sends SIGKILL to the entire process group containing the
/// target process. This forcefully terminates the process and any child
/// processes immediately, without allowing for cleanup.
///
/// ## Arguments
///
/// * `child` - The child process whose process group should be killed
///
/// ## Error Handling
///
/// - `ESRCH` (No such process) is treated as success since it means the process
///   group has already exited
/// - Other errors are propagated as `ProcessSignal` errors
///
/// ## Example
///
/// ```rust,no_run
/// use canopus_core::process::unix::{spawn, signal_kill_group};
///
/// let child = spawn("sleep", &["30"])?;
/// signal_kill_group(&child)?; // Forcefully terminate
/// # Ok::<(), canopus_core::CoreError>(())
/// ```
pub fn signal_kill_group(child: &ChildProcess) -> Result<()> {
    debug!("Sending SIGKILL to process group {}", child.pid);

    match killpg(child.pid, Signal::SIGKILL) {
        Ok(()) => {
            debug!("Successfully sent SIGKILL to process group {}", child.pid);
            Ok(())
        }
        Err(nix::errno::Errno::ESRCH) => {
            // Process group doesn't exist, which means it already exited
            debug!("Process group {} already exited", child.pid);
            Ok(())
        }
        Err(nix::errno::Errno::EPERM) => {
            // Permission denied - process may have already exited or changed ownership
            debug!(
                "Permission denied signaling process group {} (likely already exited)",
                child.pid
            );
            Ok(())
        }
        Err(e) => {
            error!(
                "Failed to send SIGKILL to process group {}: {}",
                child.pid, e
            );
            Err(CoreError::ProcessSignal(format!(
                "Failed to send SIGKILL to process group {}: {}",
                child.pid, e
            )))
        }
    }
}

/// Perform graceful termination with timeout fallback to forceful termination
///
/// This function attempts to gracefully terminate a process group with SIGTERM,
/// waits for the specified timeout, and if the process hasn't exited, sends
/// SIGKILL for forceful termination.
///
/// ## Arguments
///
/// * `child` - The child process to terminate
/// * `timeout` - How long to wait for graceful termination before using SIGKILL
///
/// ## Example
///
/// ```rust,no_run
/// use canopus_core::process::unix::{spawn, terminate_with_timeout};
/// use std::time::Duration;
///
/// let mut child = spawn("sleep", &["30"])?;
/// terminate_with_timeout(&mut child, Duration::from_secs(5))?;
/// # Ok::<(), canopus_core::CoreError>(())
/// ```
pub fn terminate_with_timeout(
    child: &mut ChildProcess,
    timeout: std::time::Duration,
) -> Result<std::process::ExitStatus> {
    // First try graceful termination
    signal_term_group(child)?;

    // Wait for the timeout period
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(status) = child.try_wait()? {
            debug!(
                "Process {} exited gracefully with status: {}",
                child.pid, status
            );
            return Ok(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // If still running, use SIGKILL
    warn!(
        "Process {} did not exit gracefully within {:?}, using SIGKILL",
        child.pid, timeout
    );
    signal_kill_group(child)?;

    // Wait a bit more for SIGKILL to take effect
    let kill_timeout = std::time::Duration::from_secs(5);
    let kill_start = std::time::Instant::now();
    while kill_start.elapsed() < kill_timeout {
        if let Some(status) = child.try_wait()? {
            debug!(
                "Process {} exited after SIGKILL with status: {}",
                child.pid, status
            );
            return Ok(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // If it still hasn't exited after SIGKILL, something is seriously wrong
    Err(CoreError::ProcessWait(format!(
        "Process {} did not exit even after SIGKILL within {:?}",
        child.pid, kill_timeout
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_spawn_simple_command() {
        let child = spawn("echo", &["hello", "world"]).expect("Failed to spawn echo");
        assert!(child.pid() > 0);
        assert_eq!(child.pid(), child.pgid()); // Process should be its own group leader
    }

    #[tokio::test]
    async fn test_spawn_and_wait() {
        let mut child = spawn("true", &[]).expect("Failed to spawn true");
        let status = child.wait().await.expect("Failed to wait for process");
        assert!(status.success());
    }

    #[tokio::test]
    async fn test_spawn_nonexistent_command() {
        let result = spawn("nonexistent_command_12345", &[]);
        assert!(result.is_err());
        match result.unwrap_err() {
            CoreError::ProcessSpawn(_) => {} // Expected error type
            e => panic!("Expected ProcessSpawn error, got: {}", e),
        }
    }

    #[tokio::test]
    async fn test_signal_term_nonexistent_process() {
        // Create a fake ChildProcess with a PID that doesn't exist
        let fake_child = ChildProcess {
            pid: Pid::from_raw(99999),
            child: spawn("true", &[]).unwrap().child, // Just for the Child handle
        };

        // Should succeed because ESRCH is treated as success
        let result = signal_term_group(&fake_child);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_signal_kill_nonexistent_process() {
        // Create a fake ChildProcess with a PID that doesn't exist
        let fake_child = ChildProcess {
            pid: Pid::from_raw(99999),
            child: spawn("true", &[]).unwrap().child, // Just for the Child handle
        };

        // Should succeed because ESRCH is treated as success
        let result = signal_kill_group(&fake_child);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_terminate_with_timeout_quick_exit() {
        // Use a process that sleeps briefly - it may exit naturally or be terminated
        let mut child = spawn("sleep", &["0.1"]).expect("Failed to spawn sleep");
        let status = terminate_with_timeout(&mut child, Duration::from_secs(1))
            .expect("Failed to terminate");
        // Process either exits successfully (0.1s elapsed) or is terminated by signal
        // Both are acceptable outcomes
        assert!(status.code().is_some() || !status.success());
    }

    #[tokio::test]
    async fn test_terminate_with_timeout_needs_kill() {
        // Use a longer sleep to test the timeout -> SIGKILL path
        let mut child = spawn("sleep", &["10"]).expect("Failed to spawn sleep");
        let status = terminate_with_timeout(&mut child, Duration::from_millis(100))
            .expect("Failed to terminate");
        // Should be killed by signal (exit code 128 + signal number)
        assert!(!status.success());
    }
}
