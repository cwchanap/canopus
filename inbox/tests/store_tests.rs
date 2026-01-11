use canopus_inbox::item::{InboxFilter, InboxStatus, NewInboxItem, SourceAgent};
use canopus_inbox::store::SqliteStore;
use canopus_inbox::InboxStore;

#[tokio::test]
async fn test_insert_and_get() {
    let store = SqliteStore::open_memory().expect("open memory db");

    let new_item = NewInboxItem::new(
        "test-project",
        "Tests passing",
        "Review PR",
        SourceAgent::ClaudeCode,
    );

    let item = store.insert(new_item).await.expect("insert");
    assert!(!item.id.is_empty());
    assert_eq!(item.project_name, "test-project");
    assert_eq!(item.status, InboxStatus::Unread);

    let retrieved = store.get(&item.id).await.expect("get").expect("found");
    assert_eq!(retrieved.id, item.id);
    assert_eq!(retrieved.project_name, "test-project");
}

#[tokio::test]
async fn test_list_with_filter() {
    let store = SqliteStore::open_memory().expect("open memory db");

    store
        .insert(NewInboxItem::new(
            "proj1",
            "s1",
            "a1",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");
    store
        .insert(NewInboxItem::new("proj2", "s2", "a2", SourceAgent::Codex))
        .await
        .expect("insert");

    let all = store.list(InboxFilter::default()).await.expect("list");
    assert_eq!(all.len(), 2);

    let claude_only = store
        .list(InboxFilter {
            source_agent: Some(SourceAgent::ClaudeCode),
            ..Default::default()
        })
        .await
        .expect("list");
    assert_eq!(claude_only.len(), 1);
    assert_eq!(claude_only[0].source_agent, SourceAgent::ClaudeCode);
}

#[tokio::test]
async fn test_mark_read_and_dismiss() {
    let store = SqliteStore::open_memory().expect("open memory db");

    let item = store
        .insert(NewInboxItem::new(
            "proj",
            "status",
            "action",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");

    assert_eq!(item.status, InboxStatus::Unread);

    store.mark_read(&item.id).await.expect("mark read");
    let updated = store.get(&item.id).await.expect("get").expect("found");
    assert_eq!(updated.status, InboxStatus::Read);

    store.dismiss(&item.id).await.expect("dismiss");
    let dismissed = store.get(&item.id).await.expect("get").expect("found");
    assert_eq!(dismissed.status, InboxStatus::Dismissed);
    assert!(dismissed.dismissed_at.is_some());
}

#[tokio::test]
async fn test_count() {
    let store = SqliteStore::open_memory().expect("open memory db");

    store
        .insert(NewInboxItem::new("p1", "s1", "a1", SourceAgent::ClaudeCode))
        .await
        .expect("insert");
    store
        .insert(NewInboxItem::new("p2", "s2", "a2", SourceAgent::ClaudeCode))
        .await
        .expect("insert");

    let count = store.count(InboxFilter::default()).await.expect("count");
    assert_eq!(count, 2);

    let unread_count = store
        .count(InboxFilter {
            status: Some(InboxStatus::Unread),
            ..Default::default()
        })
        .await
        .expect("count");
    assert_eq!(unread_count, 2);
}

#[test]
fn test_escape_like_pattern() {
    assert_eq!(canopus_inbox::store::escape_like_pattern("test"), "test");
    assert_eq!(
        canopus_inbox::store::escape_like_pattern("test%"),
        "test\\%"
    );
    assert_eq!(
        canopus_inbox::store::escape_like_pattern("test_"),
        "test\\_"
    );
    assert_eq!(
        canopus_inbox::store::escape_like_pattern("test\\"),
        "test\\\\"
    );
    assert_eq!(
        canopus_inbox::store::escape_like_pattern("100%_done\\"),
        "100\\%\\_done\\\\"
    );
    assert_eq!(canopus_inbox::store::escape_like_pattern(""), "");
}

#[tokio::test]
async fn test_sql_injection_protection() {
    let store = SqliteStore::open_memory().expect("open memory db");

    store
        .insert(NewInboxItem::new(
            "test%project",
            "s",
            "a",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");
    store
        .insert(NewInboxItem::new(
            "test_project",
            "s",
            "a",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");
    store
        .insert(NewInboxItem::new(
            "normal-project",
            "s",
            "a",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");

    let results = store
        .list(InboxFilter {
            project: Some("%".to_string()),
            ..Default::default()
        })
        .await
        .expect("list");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].project_name, "test%project");

    let results = store
        .list(InboxFilter {
            project: Some("_".to_string()),
            ..Default::default()
        })
        .await
        .expect("list");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].project_name, "test_project");
}

#[tokio::test]
async fn test_concurrent_insert() {
    let store = SqliteStore::open_memory().expect("open memory db");

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let store_clone = store.clone();
            tokio::spawn(async move {
                store_clone
                    .insert(NewInboxItem::new(
                        format!("proj-{i}"),
                        "s",
                        "a",
                        SourceAgent::ClaudeCode,
                    ))
                    .await
            })
        })
        .collect();

    for handle in handles {
        handle.await.expect("join").expect("insert");
    }

    let count = store.count(InboxFilter::default()).await.expect("count");
    assert_eq!(count, 10);
}

#[tokio::test]
async fn test_cleanup_older_than() {
    let store = SqliteStore::open_memory().expect("open memory db");

    let item = store
        .insert(NewInboxItem::new("old", "s", "a", SourceAgent::ClaudeCode))
        .await
        .expect("insert");

    let deleted = store.cleanup_older_than(-1).await.expect("cleanup");
    assert_eq!(deleted, 1);

    let result = store.get(&item.id).await.expect("get");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_cleanup_keeps_recent() {
    let store = SqliteStore::open_memory().expect("open memory db");

    store
        .insert(NewInboxItem::new(
            "recent",
            "s",
            "a",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");

    let deleted = store.cleanup_older_than(7).await.expect("cleanup");
    assert_eq!(deleted, 0);

    let count = store.count(InboxFilter::default()).await.expect("count");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_mark_notified() {
    let store = SqliteStore::open_memory().expect("open memory db");

    let item = store
        .insert(NewInboxItem::new("p", "s", "a", SourceAgent::ClaudeCode))
        .await
        .expect("insert");

    assert!(!item.notified);

    store.mark_notified(&item.id).await.expect("mark notified");

    let updated = store.get(&item.id).await.expect("get").expect("found");
    assert!(updated.notified);
}

#[tokio::test]
async fn test_filter_combinations() {
    let store = SqliteStore::open_memory().expect("open memory db");

    store
        .insert(NewInboxItem::new(
            "alpha-1",
            "s",
            "a",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");
    store
        .insert(NewInboxItem::new(
            "alpha-2",
            "s",
            "a",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");
    store
        .insert(NewInboxItem::new("beta", "s", "a", SourceAgent::Codex))
        .await
        .expect("insert");

    let results = store
        .list(InboxFilter {
            project: Some("alpha".to_string()),
            source_agent: Some(SourceAgent::ClaudeCode),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .expect("list");

    assert_eq!(results.len(), 1);
    assert!(results[0].project_name.contains("alpha"));
    assert_eq!(results[0].source_agent, SourceAgent::ClaudeCode);
}

#[tokio::test]
async fn test_project_partial_match() {
    let store = SqliteStore::open_memory().expect("open memory db");

    store
        .insert(NewInboxItem::new(
            "my-awesome-project",
            "s",
            "a",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");

    let results = store
        .list(InboxFilter {
            project: Some("awesome".to_string()),
            ..Default::default()
        })
        .await
        .expect("list");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].project_name, "my-awesome-project");
}

#[tokio::test]
async fn test_dismiss_nonexistent() {
    let store = SqliteStore::open_memory().expect("open memory db");

    let result = store.dismiss("non-existent-id").await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        canopus_inbox::InboxError::NotFound(_)
    ));
}

#[tokio::test]
async fn test_mark_read_idempotent() {
    let store = SqliteStore::open_memory().expect("open memory db");

    let item = store
        .insert(NewInboxItem::new("p", "s", "a", SourceAgent::ClaudeCode))
        .await
        .expect("insert");

    store.mark_read(&item.id).await.expect("first read");
    store.mark_read(&item.id).await.expect("second read");

    let updated = store.get(&item.id).await.expect("get").expect("found");
    assert_eq!(updated.status, InboxStatus::Read);
}

#[tokio::test]
async fn test_get_nonexistent() {
    let store = SqliteStore::open_memory().expect("open memory db");

    let result = store.get("non-existent-id").await.expect("get");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_list_empty_database() {
    let store = SqliteStore::open_memory().expect("open memory db");

    let results = store.list(InboxFilter::default()).await.expect("list");
    assert!(results.is_empty());

    let count = store.count(InboxFilter::default()).await.expect("count");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_count_with_project_filter() {
    let store = SqliteStore::open_memory().expect("open memory db");

    store
        .insert(NewInboxItem::new(
            "alpha",
            "s",
            "a",
            SourceAgent::ClaudeCode,
        ))
        .await
        .expect("insert");
    store
        .insert(NewInboxItem::new("beta", "s", "a", SourceAgent::ClaudeCode))
        .await
        .expect("insert");

    let count = store
        .count(InboxFilter {
            project: Some("alpha".to_string()),
            ..Default::default()
        })
        .await
        .expect("count");
    assert_eq!(count, 1);
}
