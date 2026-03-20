//! Logging utilities: bounded ring buffer with drop counters and snapshot/iterator APIs

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A single log entry captured from a service's stdout/stderr
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// Monotonic sequence number for the entry
    pub seq: u64,
    /// Stream kind (stdout or stderr)
    pub stream: schema::LogStream,
    /// Raw content of the log line, without trailing newline
    pub content: String,
    /// Timestamp in RFC3339 format
    pub timestamp: String,
}

/// A bounded-capacity ring buffer to store recent log entries.
///
/// - When capacity is exceeded, oldest entries are dropped and `total_dropped` is incremented.
/// - Sequence numbers increase monotonically and are attached to each entry when pushed.
#[derive(Debug)]
pub struct LogRing {
    capacity: usize,
    total_dropped: u64,
    next_seq: u64,
    entries: VecDeque<LogEntry>,
}

impl LogRing {
    /// Create a new `LogRing` with the given capacity (must be > 0)
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "LogRing capacity must be > 0");
        Self {
            capacity,
            total_dropped: 0,
            next_seq: 0,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    /// Push a new entry into the ring, assigning the next sequence number.
    /// If the ring is full, evicts the oldest entry and increments the drop counter.
    pub fn push(&mut self, mut entry: LogEntry) {
        // assign seq
        entry.seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);

        if self.entries.len() == self.capacity {
            self.entries.pop_front();
            self.total_dropped = self.total_dropped.saturating_add(1);
        }
        self.entries.push_back(entry);
    }

    /// Current number of entries retained
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the ring is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total number of entries ever dropped due to capacity
    #[must_use]
    pub const fn total_dropped(&self) -> u64 {
        self.total_dropped
    }

    /// Current high-water sequence value (the next seq to be assigned)
    #[must_use]
    pub const fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Snapshot the current entries. Returns (`next_seq`, `entries_clone`)
    /// where `next_seq` can be used to resume with `iter_after` later.
    #[must_use]
    pub fn snapshot(&self) -> (u64, Vec<LogEntry>) {
        (self.next_seq, self.entries.iter().cloned().collect())
    }

    /// Returns all entries with seq strictly greater than `after_seq`.
    /// This is useful for tailing starting from a previously observed sequence.
    #[must_use]
    pub fn iter_after(&self, after_seq: u64) -> Vec<LogEntry> {
        self.entries
            .iter()
            .filter(|e| e.seq > after_seq)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_entry(stream: schema::LogStream, content: &str) -> LogEntry {
        LogEntry {
            seq: 0,
            stream,
            content: content.to_string(),
            timestamp: schema::ServiceEvent::current_timestamp(),
        }
    }

    #[test]
    fn test_wrap_around_and_drop_count() {
        let mut ring = LogRing::new(3);
        assert_eq!(ring.len(), 0);
        assert_eq!(ring.total_dropped(), 0);

        // push 5 entries into capacity 3 => 2 dropped overall
        ring.push(mk_entry(schema::LogStream::Stdout, "a"));
        ring.push(mk_entry(schema::LogStream::Stdout, "b"));
        ring.push(mk_entry(schema::LogStream::Stdout, "c"));
        ring.push(mk_entry(schema::LogStream::Stdout, "d"));
        ring.push(mk_entry(schema::LogStream::Stdout, "e"));

        assert_eq!(ring.len(), 3);
        assert_eq!(ring.total_dropped(), 2);

        // entries should be c, d, e with seq 2,3,4
        let (_next, snap) = ring.snapshot();
        let contents: Vec<_> = snap.iter().map(|e| e.content.as_str()).collect();
        assert_eq!(contents, vec!["c", "d", "e"]);
        let seqs: Vec<_> = snap.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![2, 3, 4]);
    }

    #[test]
    fn test_iter_after() {
        let mut ring = LogRing::new(3);
        ring.push(mk_entry(schema::LogStream::Stdout, "a")); // seq 0
        ring.push(mk_entry(schema::LogStream::Stdout, "b")); // seq 1
        ring.push(mk_entry(schema::LogStream::Stdout, "c")); // seq 2
        let v = ring.iter_after(0);
        let contents: Vec<_> = v.iter().map(|e| e.content.as_str()).collect();
        assert_eq!(contents, vec!["b", "c"]);

        // cause wrap
        ring.push(mk_entry(schema::LogStream::Stdout, "d")); // evict a, seq 3
        let v2 = ring.iter_after(1);
        let contents2: Vec<_> = v2.iter().map(|e| e.content.as_str()).collect();
        assert_eq!(contents2, vec!["c", "d"]);
    }

    #[test]
    fn test_is_empty_and_len() {
        let mut ring = LogRing::new(5);
        assert!(ring.is_empty());
        assert_eq!(ring.len(), 0);

        ring.push(mk_entry(schema::LogStream::Stdout, "x"));
        assert!(!ring.is_empty());
        assert_eq!(ring.len(), 1);
    }

    #[test]
    fn test_next_seq_advances() {
        let mut ring = LogRing::new(4);
        assert_eq!(ring.next_seq(), 0);

        ring.push(mk_entry(schema::LogStream::Stdout, "a"));
        assert_eq!(ring.next_seq(), 1);

        ring.push(mk_entry(schema::LogStream::Stdout, "b"));
        assert_eq!(ring.next_seq(), 2);
    }

    #[test]
    fn test_next_seq_after_overflow() {
        let mut ring = LogRing::new(2);
        // Push 5 items: ring wraps, but seq keeps incrementing
        for i in 0..5u64 {
            ring.push(mk_entry(schema::LogStream::Stdout, &i.to_string()));
        }
        assert_eq!(ring.next_seq(), 5);
        assert_eq!(ring.total_dropped(), 3); // capacity 2, 5 pushed, 3 dropped
    }

    #[test]
    fn test_snapshot_returns_current_entries_and_next_seq() {
        let mut ring = LogRing::new(3);
        ring.push(mk_entry(schema::LogStream::Stderr, "err1")); // seq 0
        ring.push(mk_entry(schema::LogStream::Stdout, "out1")); // seq 1

        let (next_seq, entries) = ring.snapshot();
        assert_eq!(next_seq, 2);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].content, "err1");
        assert_eq!(entries[0].stream, schema::LogStream::Stderr);
        assert_eq!(entries[1].content, "out1");
    }

    #[test]
    fn test_iter_after_returns_empty_when_no_newer_entries() {
        let mut ring = LogRing::new(3);
        ring.push(mk_entry(schema::LogStream::Stdout, "a")); // seq 0
        ring.push(mk_entry(schema::LogStream::Stdout, "b")); // seq 1

        // iter_after the last seq (1) returns nothing
        let v = ring.iter_after(1);
        assert!(v.is_empty());

        // iter_after a very large seq also returns nothing
        let v2 = ring.iter_after(u64::MAX - 1);
        assert!(v2.is_empty());
    }

    #[test]
    fn test_single_capacity_ring() {
        let mut ring = LogRing::new(1);
        assert!(ring.is_empty());

        ring.push(mk_entry(schema::LogStream::Stdout, "first"));
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.total_dropped(), 0);

        ring.push(mk_entry(schema::LogStream::Stdout, "second"));
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.total_dropped(), 1);

        let (_, snap) = ring.snapshot();
        assert_eq!(snap[0].content, "second");
        assert_eq!(snap[0].seq, 1);
    }

    #[test]
    #[should_panic(expected = "LogRing capacity must be > 0")]
    fn test_zero_capacity_panics() {
        let _ring = LogRing::new(0);
    }

    #[test]
    fn test_log_entry_fields_preserved() {
        let mut ring = LogRing::new(10);
        let ts = schema::ServiceEvent::current_timestamp();
        let entry = LogEntry {
            seq: 0, // will be overwritten
            stream: schema::LogStream::Stderr,
            content: "important error".to_string(),
            timestamp: ts.clone(),
        };
        ring.push(entry);

        let (_, snap) = ring.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].seq, 0); // assigned by ring
        assert_eq!(snap[0].stream, schema::LogStream::Stderr);
        assert_eq!(snap[0].content, "important error");
        assert_eq!(snap[0].timestamp, ts);
    }
}
