#![allow(missing_docs, unused_crate_dependencies)]

use canopus_inbox::item::{InboxItem, NewInboxItem, SourceAgent};
use canopus_inbox::notify::{format_item_notification, format_summary_notification, truncate};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug, Clone, Default)]
struct MockNotifier {
    call_count: Arc<AtomicUsize>,
    summaries: Arc<Mutex<Vec<String>>>,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl MockNotifier {
    fn new() -> Self {
        Self::default()
    }

    fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    fn summaries(&self) -> Vec<String> {
        self.summaries
            .lock()
            .map_or_else(|_| Vec::new(), |summaries| summaries.clone())
    }

    fn bodies(&self) -> Vec<String> {
        self.bodies
            .lock()
            .map_or_else(|_| Vec::new(), |bodies| bodies.clone())
    }
}

impl canopus_inbox::notify::NotificationBackend for MockNotifier {
    fn send(&self, summary: &str, body: &str) -> canopus_inbox::Result<()> {
        let mut summaries = self.summaries.lock().map_err(|_| {
            canopus_inbox::InboxError::Notification(
                "Notification summary lock poisoned".to_string(),
            )
        })?;
        let mut bodies = self.bodies.lock().map_err(|_| {
            canopus_inbox::InboxError::Notification(
                "Notification body lock poisoned".to_string(),
            )
        })?;
        summaries.push(summary.to_string());
        bodies.push(body.to_string());
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn test_truncate() {
    assert_eq!(truncate("hello", 10), "hello");
    assert_eq!(truncate("hello world", 8), "hello...");
}

#[test]
fn test_truncate_unicode() {
    assert_eq!(truncate("Hello 世界", 20), "Hello 世界");
    assert_eq!(truncate("Hello 世界!", 8), "Hello...");
    assert_eq!(truncate("Test 🚀🎉", 10), "Test 🚀🎉");
    assert_eq!(truncate("Test 🚀🎉 more", 8), "Test ...");
}

#[test]
fn test_truncate_edge_cases() {
    assert_eq!(truncate("", 10), "");
    assert_eq!(truncate("a", 1), "a");
    assert_eq!(truncate("abc", 3), "abc");
    assert_eq!(truncate("abcd", 3), "...");
    assert_eq!(truncate("hello", 4), "h...");
}

#[test]
fn test_truncate_very_long() {
    let long_str = "a".repeat(1000);
    let result = truncate(&long_str, 50);
    assert_eq!(result.chars().count(), 50);
    assert!(result.ends_with("..."));
}

#[test]
fn test_format_item_notification() {
    let item = InboxItem::from_new(NewInboxItem::new(
        "my-project",
        "All tests passing",
        "Review and merge PR",
        SourceAgent::ClaudeCode,
    ));

    let (summary, body) = format_item_notification(&item);

    assert_eq!(summary, "[Claude Code] my-project");
    assert!(body.contains("All tests passing"));
    assert!(body.contains("Review and merge PR"));
}

#[test]
fn test_format_summary_notification_empty() {
    let (summary, body) = format_summary_notification(0, &[]);

    assert_eq!(summary, "Canopus Inbox: 0 new item(s)");
    assert_eq!(body, "Check your inbox for details.");
}

#[test]
fn test_format_summary_notification_multiple() {
    let items = vec![
        InboxItem::from_new(NewInboxItem::new("p1", "s1", "a1", SourceAgent::ClaudeCode)),
        InboxItem::from_new(NewInboxItem::new("p2", "s2", "a2", SourceAgent::Codex)),
        InboxItem::from_new(NewInboxItem::new("p3", "s3", "a3", SourceAgent::Windsurf)),
        InboxItem::from_new(NewInboxItem::new("p4", "s4", "a4", SourceAgent::OpenCode)),
    ];

    let (summary, body) = format_summary_notification(4, &items);

    assert_eq!(summary, "Canopus Inbox: 4 new item(s)");
    assert!(body.contains("[Claude Code] p1"));
    assert!(body.contains("[Codex CLI] p2"));
    assert!(body.contains("[Windsurf] p3"));
    assert!(!body.contains("p4"));
}

#[test]
fn test_notifier_send_notification() {
    let mock = MockNotifier::new();
    let notifier = canopus_inbox::notify::Notifier::new(mock.clone());

    let item = InboxItem::from_new(NewInboxItem::new(
        "test-project",
        "Status summary",
        "Action required",
        SourceAgent::ClaudeCode,
    ));

    assert!(notifier.send_notification(&item).is_ok());

    assert_eq!(mock.call_count(), 1);
    let summaries = mock.summaries();
    assert_eq!(
        summaries.first().map(String::as_str),
        Some("[Claude Code] test-project")
    );
    let bodies = mock.bodies();
    assert!(bodies
        .first()
        .is_some_and(|body| body.contains("Status summary")));
}

#[test]
fn test_notifier_send_summary_notification() {
    let mock = MockNotifier::new();
    let notifier = canopus_inbox::notify::Notifier::new(mock.clone());

    let items = vec![
        InboxItem::from_new(NewInboxItem::new("p1", "s1", "a1", SourceAgent::ClaudeCode)),
        InboxItem::from_new(NewInboxItem::new("p2", "s2", "a2", SourceAgent::Codex)),
    ];

    assert!(notifier.send_summary_notification(2, &items).is_ok());

    assert_eq!(mock.call_count(), 1);
    let summaries = mock.summaries();
    assert!(summaries
        .first()
        .is_some_and(|summary| summary.contains("2 new item(s)")));
}

#[test]
fn test_notifier_with_long_content() {
    let mock = MockNotifier::new();
    let notifier = canopus_inbox::notify::Notifier::new(mock.clone());

    let item = InboxItem::from_new(NewInboxItem::new(
        "very-long-project-name",
        "a".repeat(200),
        "b".repeat(200),
        SourceAgent::ClaudeCode,
    ));

    assert!(notifier.send_notification(&item).is_ok());

    assert_eq!(mock.call_count(), 1);
    let bodies = mock.bodies();
    assert!(bodies.first().is_some_and(|body| body.len() < 400));
}

#[test]
fn test_notifier_multiple_calls() {
    let mock = MockNotifier::new();
    let notifier = canopus_inbox::notify::Notifier::new(mock.clone());

    for i in 0..5 {
        let item = InboxItem::from_new(NewInboxItem::new(
            format!("project-{i}"),
            "status",
            "action",
            SourceAgent::ClaudeCode,
        ));
        assert!(notifier.send_notification(&item).is_ok());
    }

    assert_eq!(mock.call_count(), 5);
    assert_eq!(mock.summaries().len(), 5);
}
