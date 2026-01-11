//! Desktop notification support for inbox items.

use crate::error::{InboxError, Result};
use crate::item::InboxItem;
use tracing::{debug, warn};

/// Trait for notification backends, allowing for mocking in tests.
pub trait NotificationBackend: Send + Sync {
    /// Send a notification with the given summary and body.
    fn send(&self, summary: &str, body: &str) -> Result<()>;
}

/// Desktop notification backend using notify-rust.
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopNotifier;

impl NotificationBackend for DesktopNotifier {
    fn send(&self, summary: &str, body: &str) -> Result<()> {
        use notify_rust::Notification;

        debug!("Sending notification: {}", summary);

        Notification::new()
            .appname("Canopus Inbox")
            .summary(summary)
            .body(body)
            .icon("mail-unread")
            .timeout(notify_rust::Timeout::Milliseconds(5000))
            .show()
            .map_err(|e| {
                warn!("Failed to send notification: {}", e);
                InboxError::Notification(e.to_string())
            })?;

        Ok(())
    }
}

/// Notifier that formats and sends inbox notifications.
#[derive(Debug)]
pub struct Notifier<B: NotificationBackend = DesktopNotifier> {
    backend: B,
}

impl Default for Notifier<DesktopNotifier> {
    fn default() -> Self {
        Self::new(DesktopNotifier)
    }
}

impl<B: NotificationBackend> Notifier<B> {
    /// Creates a new notifier with the given backend.
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Sends a desktop notification for an inbox item.
    ///
    /// # Errors
    ///
    /// Returns an error if the notification cannot be sent.
    pub fn send_notification(&self, item: &InboxItem) -> Result<()> {
        let (summary, body) = format_item_notification(item);
        self.backend.send(&summary, &body)
    }

    /// Sends a summary notification for multiple items.
    ///
    /// # Errors
    ///
    /// Returns an error if the notification cannot be sent.
    pub fn send_summary_notification(&self, count: usize, items: &[InboxItem]) -> Result<()> {
        let (summary, body) = format_summary_notification(count, items);
        self.backend.send(&summary, &body)
    }
}

/// Formats an inbox item into notification summary and body.
#[must_use]
pub fn format_item_notification(item: &InboxItem) -> (String, String) {
    let summary = format!(
        "[{}] {}",
        item.source_agent.display_name(),
        item.project_name
    );

    let body = format!(
        "{}\n\nAction: {}",
        truncate(&item.status_summary, 100),
        truncate(&item.action_required, 100)
    );

    (summary, body)
}

/// Formats a summary notification for multiple items.
#[must_use]
pub fn format_summary_notification(count: usize, items: &[InboxItem]) -> (String, String) {
    let summary = format!("Canopus Inbox: {count} new item(s)");

    let body = if items.is_empty() {
        "Check your inbox for details.".to_string()
    } else {
        items
            .iter()
            .take(3)
            .map(|item| {
                format!(
                    "• [{}] {}",
                    item.source_agent.display_name(),
                    item.project_name
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    (summary, body)
}

// --- Convenience functions using default desktop notifier ---

/// Sends a desktop notification for an inbox item.
///
/// Uses native notification mechanisms:
/// - macOS: via `notify-rust` (which uses `osascript`)
/// - Linux: via `notify-rust` (which uses `notify-send` or D-Bus)
///
/// # Errors
///
/// Returns an error if the notification cannot be sent.
pub fn send_notification(item: &InboxItem) -> Result<()> {
    Notifier::default().send_notification(item)
}

/// Sends a summary notification for multiple items.
///
/// # Errors
///
/// Returns an error if the notification cannot be sent.
pub fn send_summary_notification(count: usize, items: &[InboxItem]) -> Result<()> {
    Notifier::default().send_summary_notification(count, items)
}

/// Check if desktop notifications are available on this system.
#[must_use]
#[allow(clippy::missing_const_for_fn)] // Not const on Linux (runs command)
pub fn is_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        true // macOS always has notification support
    }

    #[cfg(target_os = "linux")]
    {
        // Check for notification daemon
        std::process::Command::new("notify-send")
            .arg("--version")
            .output()
            .is_ok()
    }

    #[cfg(target_os = "windows")]
    {
        true // Windows 10+ has notification support
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

/// Truncates a string to a maximum length, adding "..." if truncated.
/// Handles Unicode correctly by respecting character boundaries.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncate_at = max_len.saturating_sub(3);
        let truncated: String = s.chars().take(truncate_at).collect();
        format!("{truncated}...")
    }
}
