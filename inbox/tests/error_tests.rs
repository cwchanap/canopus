use canopus_inbox::error::{InboxError, Result};

#[test]
fn test_error_codes() {
    assert_eq!(InboxError::NotFound("test".into()).code(), "INBOX001");
    assert_eq!(InboxError::Database("test".into()).code(), "INBOX002");
    assert_eq!(InboxError::Serialization("test".into()).code(), "INBOX003");
    assert_eq!(InboxError::Notification("test".into()).code(), "INBOX004");
    assert_eq!(InboxError::InvalidInput("test".into()).code(), "INBOX005");
    assert_eq!(InboxError::Io("test".into()).code(), "INBOX006");
}

#[test]
fn test_error_display() {
    let err = InboxError::NotFound("test-id".into());
    assert!(err.to_string().contains("test-id"));
    assert!(err.to_string().contains("not found"));

    let err = InboxError::Database("connection failed".into());
    assert!(err.to_string().contains("connection failed"));
    assert!(err.to_string().contains("Database"));

    let err = InboxError::Serialization("invalid json".into());
    assert!(err.to_string().contains("invalid json"));

    let err = InboxError::Notification("daemon unavailable".into());
    assert!(err.to_string().contains("daemon unavailable"));

    let err = InboxError::InvalidInput("bad value".into());
    assert!(err.to_string().contains("bad value"));

    let err = InboxError::Io("file not found".into());
    assert!(err.to_string().contains("file not found"));
}

#[test]
fn test_error_from_rusqlite() {
    let sqlite_err = rusqlite::Error::InvalidQuery;
    let inbox_err = InboxError::from(sqlite_err);

    assert!(matches!(inbox_err, InboxError::Database(_)));
    assert_eq!(inbox_err.code(), "INBOX002");
    assert!(inbox_err.to_string().contains("Database"));
}

#[test]
fn test_error_from_serde_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("{invalid").unwrap_err();
    let inbox_err = InboxError::from(json_err);

    assert!(matches!(inbox_err, InboxError::Serialization(_)));
    assert_eq!(inbox_err.code(), "INBOX003");
}

#[test]
fn test_error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test file");
    let inbox_err = InboxError::from(io_err);

    assert!(matches!(inbox_err, InboxError::Io(_)));
    assert_eq!(inbox_err.code(), "INBOX006");
    assert!(inbox_err.to_string().contains("test file"));
}

#[test]
fn test_error_debug() {
    let err = InboxError::NotFound("test".into());
    let debug_str = format!("{:?}", err);
    assert!(debug_str.contains("NotFound"));
    assert!(debug_str.contains("test"));
}

#[test]
fn test_result_type_alias() {
    let err_result: Result<i32> = Err(InboxError::NotFound("missing".into()));
    assert!(err_result.is_err());

    let ok_result: Result<i32> = Ok(42);
    assert!(ok_result.is_ok());
}
