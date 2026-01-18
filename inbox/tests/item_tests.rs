#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::unwrap_used,
    unused_crate_dependencies
)]

use canopus_inbox::item::{InboxFilter, InboxItem, InboxStatus, NewInboxItem, SourceAgent};

#[test]
fn test_source_agent_parsing() {
    assert_eq!(
        SourceAgent::from_str("claude-code"),
        SourceAgent::ClaudeCode
    );
    assert_eq!(SourceAgent::from_str("CLAUDE"), SourceAgent::ClaudeCode);
    assert_eq!(SourceAgent::from_str("codex"), SourceAgent::Codex);
    assert_eq!(SourceAgent::from_str("windsurf"), SourceAgent::Windsurf);
    assert_eq!(SourceAgent::from_str("opencode"), SourceAgent::OpenCode);
    assert_eq!(SourceAgent::from_str("unknown"), SourceAgent::Other);
}

#[test]
fn test_inbox_status_parsing() {
    assert_eq!(InboxStatus::from_str("unread"), InboxStatus::Unread);
    assert_eq!(InboxStatus::from_str("read"), InboxStatus::Read);
    assert_eq!(InboxStatus::from_str("dismissed"), InboxStatus::Dismissed);
    assert_eq!(InboxStatus::from_str("invalid"), InboxStatus::Unread);
}

#[test]
fn test_new_inbox_item_creation() {
    let new_item = NewInboxItem::new(
        "my-project",
        "Tests passing",
        "Review PR #123",
        SourceAgent::ClaudeCode,
    );

    let item = InboxItem::from_new(new_item);
    assert!(!item.id.is_empty());
    assert_eq!(item.project_name, "my-project");
    assert_eq!(item.status, InboxStatus::Unread);
    assert!(!item.notified);
}

#[test]
fn test_inbox_item_serialization() {
    let item = NewInboxItem::new("test", "status", "action", SourceAgent::ClaudeCode);

    let json = serde_json::to_string(&item).expect("serialize");
    assert!(json.contains("projectName"));
    assert!(json.contains("claudeCode"));
}

#[test]
fn test_source_agent_as_str() {
    assert_eq!(SourceAgent::ClaudeCode.as_str(), "claude-code");
    assert_eq!(SourceAgent::Codex.as_str(), "codex");
    assert_eq!(SourceAgent::Windsurf.as_str(), "windsurf");
    assert_eq!(SourceAgent::OpenCode.as_str(), "opencode");
    assert_eq!(SourceAgent::Other.as_str(), "other");
}

#[test]
fn test_inbox_status_as_str() {
    assert_eq!(InboxStatus::Unread.as_str(), "unread");
    assert_eq!(InboxStatus::Read.as_str(), "read");
    assert_eq!(InboxStatus::Dismissed.as_str(), "dismissed");
}

#[test]
fn test_source_agent_display() {
    assert_eq!(SourceAgent::ClaudeCode.to_string(), "Claude Code");
    assert_eq!(SourceAgent::Codex.to_string(), "Codex CLI");
    assert_eq!(SourceAgent::Windsurf.to_string(), "Windsurf");
    assert_eq!(SourceAgent::OpenCode.to_string(), "OpenCode");
    assert_eq!(SourceAgent::Other.to_string(), "Other");
}

#[test]
fn test_inbox_status_display() {
    assert_eq!(InboxStatus::Unread.to_string(), "unread");
    assert_eq!(InboxStatus::Read.to_string(), "read");
    assert_eq!(InboxStatus::Dismissed.to_string(), "dismissed");
}

#[test]
fn test_with_details() {
    let item =
        NewInboxItem::new("p", "s", "a", SourceAgent::ClaudeCode).with_details(serde_json::json!({
            "key": "value",
            "count": 42,
            "nested": {"inner": true}
        }));

    assert!(item.details.is_some());
    let details = item.details.expect("expected details to be present");
    assert_eq!(details.get("key").and_then(|value| value.as_str()), Some("value"));
    assert_eq!(details.get("count").and_then(serde_json::Value::as_i64), Some(42));
    assert_eq!(
        details
            .get("nested")
            .and_then(|value| value.get("inner"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn test_age_display_just_now() {
    let item = InboxItem::from_new(NewInboxItem::new("p", "s", "a", SourceAgent::ClaudeCode));
    assert_eq!(item.age_display(), "just now");
}

#[test]
fn test_inbox_item_full_roundtrip() {
    let new_item = NewInboxItem::new("test-project", "status", "action", SourceAgent::ClaudeCode)
        .with_details(serde_json::json!({"test": true}));

    let item = InboxItem::from_new(new_item);

    let json = serde_json::to_string(&item).expect("serialize");
    let deserialized: InboxItem = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deserialized.id, item.id);
    assert_eq!(deserialized.project_name, "test-project");
    assert_eq!(deserialized.source_agent, SourceAgent::ClaudeCode);
    assert_eq!(deserialized.status, InboxStatus::Unread);
    assert!(deserialized.details.is_some());
}

#[test]
fn test_source_agent_parsing_variations() {
    assert_eq!(
        SourceAgent::from_str("claude-code"),
        SourceAgent::ClaudeCode
    );
    assert_eq!(SourceAgent::from_str("claudecode"), SourceAgent::ClaudeCode);
    assert_eq!(SourceAgent::from_str("claude"), SourceAgent::ClaudeCode);
    assert_eq!(
        SourceAgent::from_str("CLAUDE-CODE"),
        SourceAgent::ClaudeCode
    );

    assert_eq!(SourceAgent::from_str("codex"), SourceAgent::Codex);
    assert_eq!(SourceAgent::from_str("codex-cli"), SourceAgent::Codex);
    assert_eq!(SourceAgent::from_str("CODEX"), SourceAgent::Codex);

    assert_eq!(SourceAgent::from_str("opencode"), SourceAgent::OpenCode);
    assert_eq!(SourceAgent::from_str("open-code"), SourceAgent::OpenCode);
}

#[test]
fn test_inbox_status_case_insensitive() {
    assert_eq!(InboxStatus::from_str("READ"), InboxStatus::Read);
    assert_eq!(InboxStatus::from_str("Read"), InboxStatus::Read);
    assert_eq!(InboxStatus::from_str("DISMISSED"), InboxStatus::Dismissed);
}

#[test]
fn test_inbox_filter_default() {
    let filter = InboxFilter::default();
    assert!(filter.status.is_none());
    assert!(filter.source_agent.is_none());
    assert!(filter.project.is_none());
    assert!(filter.limit.is_none());
}

#[test]
fn test_new_inbox_item_fields() {
    let item = NewInboxItem::new(
        "project-name",
        "status-summary",
        "action-required",
        SourceAgent::Windsurf,
    );

    assert_eq!(item.project_name, "project-name");
    assert_eq!(item.status_summary, "status-summary");
    assert_eq!(item.action_required, "action-required");
    assert_eq!(item.source_agent, SourceAgent::Windsurf);
    assert!(item.details.is_none());
}
