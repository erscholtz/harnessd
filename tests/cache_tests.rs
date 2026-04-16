//! Integration tests for the proposal cache module.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use harnessd::cache::{MAX_BYTES, MAX_LINES, ProposalCache, compute_hash};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_db_path() -> PathBuf {
    let temp_dir = std::env::temp_dir();
    let unique_name = format!(
        "test_proposals_{}_{}.db",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    temp_dir.join(unique_name)
}

#[tokio::test]
async fn test_cache_open_and_schema_creation() {
    let db_path = temp_db_path();

    // Create a new cache
    let cache = ProposalCache::open(&db_path).expect("failed to open cache");

    // Verify we can get stats (indicates schema was created)
    let stats = cache.stats().await.expect("failed to get stats");
    assert_eq!(stats.total_proposals, 0);

    // Cleanup
    std::fs::remove_file(&db_path).ok();
}

#[tokio::test]
async fn test_cache_store_and_lookup() {
    let db_path = temp_db_path();
    let cache = ProposalCache::open(&db_path).expect("failed to open cache");

    // Store a proposal
    let file_path = "/test/file.rs";
    let byte_start = 100;
    let byte_end = 200;
    let content = "fn test() {}";
    let content_hash = compute_hash(content);
    let snippet = "fn test() { println!(\"hello\"); }";
    let label = "test function";

    let id = cache
        .store(
            file_path,
            byte_start,
            byte_end,
            &content_hash,
            snippet,
            label,
        )
        .await
        .expect("failed to store proposal");

    assert!(id > 0);

    // Lookup the proposal
    let proposals = cache
        .lookup(file_path, byte_start, byte_end, &content_hash)
        .await
        .expect("failed to lookup proposal");

    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].file_path, file_path);
    assert_eq!(proposals[0].byte_start, byte_start);
    assert_eq!(proposals[0].byte_end, byte_end);
    assert_eq!(proposals[0].content_hash, content_hash);
    assert_eq!(proposals[0].snippet, snippet);
    assert_eq!(proposals[0].label, label);

    // Cleanup
    std::fs::remove_file(&db_path).ok();
}

#[tokio::test]
async fn test_cache_lookup_miss() {
    let db_path = temp_db_path();
    let cache = ProposalCache::open(&db_path).expect("failed to open cache");

    // Lookup non-existent proposal
    let proposals = cache
        .lookup("/nonexistent/file.rs", 0, 100, "wrong_hash")
        .await
        .expect("failed to lookup proposal");

    assert!(proposals.is_empty());

    // Cleanup
    std::fs::remove_file(&db_path).ok();
}

#[tokio::test]
async fn test_cache_lookup_at_offset() {
    let db_path = temp_db_path();
    let cache = ProposalCache::open(&db_path).expect("failed to open cache");

    // Store a proposal covering bytes 100-200
    let file_path = "/test/file.rs";
    let content = "test content";
    let content_hash = compute_hash(content);

    cache
        .store(file_path, 100, 200, &content_hash, "snippet", "label")
        .await
        .expect("failed to store proposal");

    // Lookup at offset 150 (within range)
    let proposals = cache
        .lookup_at_offset(file_path, 150)
        .await
        .expect("failed to lookup at offset");

    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].byte_start, 100);

    // Lookup at offset 50 (outside range)
    let proposals = cache
        .lookup_at_offset(file_path, 50)
        .await
        .expect("failed to lookup at offset");

    assert!(proposals.is_empty());

    // Cleanup
    std::fs::remove_file(&db_path).ok();
}

#[tokio::test]
async fn test_cache_store_enforces_max_lines() {
    let db_path = temp_db_path();
    let cache = ProposalCache::open(&db_path).expect("failed to open cache");

    // Create a snippet with too many lines
    let mut snippet = String::new();
    for i in 0..=MAX_LINES {
        snippet.push_str(&format!("line {}\n", i));
    }

    let result = cache
        .store("/test/file.rs", 0, 100, "hash", &snippet, "label")
        .await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("max lines"));

    // Cleanup
    std::fs::remove_file(&db_path).ok();
}

#[tokio::test]
async fn test_cache_store_enforces_max_bytes() {
    let db_path = temp_db_path();
    let cache = ProposalCache::open(&db_path).expect("failed to open cache");

    // Create a snippet with too many bytes
    let snippet = "x".repeat(MAX_BYTES + 1);

    let result = cache
        .store("/test/file.rs", 0, 100, "hash", &snippet, "label")
        .await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("max bytes"));

    // Cleanup
    std::fs::remove_file(&db_path).ok();
}

#[tokio::test]
async fn test_cache_invalidate_file() {
    let db_path = temp_db_path();
    let cache = ProposalCache::open(&db_path).expect("failed to open cache");

    // Store proposals for two different files
    cache
        .store("/test/file1.rs", 0, 100, "hash1", "snippet1", "label1")
        .await
        .expect("failed to store");

    cache
        .store("/test/file2.rs", 0, 100, "hash2", "snippet2", "label2")
        .await
        .expect("failed to store");

    // Invalidate file1
    let count = cache
        .invalidate_file("/test/file1.rs")
        .await
        .expect("failed to invalidate");

    assert_eq!(count, 1);

    // Verify file1 proposals are gone
    let proposals = cache
        .lookup("/test/file1.rs", 0, 100, "hash1")
        .await
        .expect("failed to lookup");
    assert!(proposals.is_empty());

    // Verify file2 proposals remain
    let proposals = cache
        .lookup("/test/file2.rs", 0, 100, "hash2")
        .await
        .expect("failed to lookup");
    assert_eq!(proposals.len(), 1);

    // Cleanup
    std::fs::remove_file(&db_path).ok();
}

#[tokio::test]
async fn test_cache_stats() {
    let db_path = temp_db_path();
    let cache = ProposalCache::open(&db_path).expect("failed to open cache");

    // Initial stats
    let stats = cache.stats().await.expect("failed to get stats");
    assert_eq!(stats.total_proposals, 0);
    assert_eq!(stats.total_bytes, 0);
    assert!(stats.oldest_timestamp.is_none());
    assert!(stats.newest_timestamp.is_none());

    // Store some proposals
    cache
        .store("/test/file.rs", 0, 100, "hash1", "snippet1", "label1")
        .await
        .expect("failed to store");

    cache
        .store(
            "/test/file.rs",
            100,
            200,
            "hash2",
            "snippet2 content",
            "label2",
        )
        .await
        .expect("failed to store");

    // Check updated stats
    let stats = cache.stats().await.expect("failed to get stats");
    assert_eq!(stats.total_proposals, 2);
    assert_eq!(
        stats.total_bytes,
        "snippet1".len() + "snippet2 content".len()
    );
    assert!(stats.oldest_timestamp.is_some());
    assert!(stats.newest_timestamp.is_some());

    // Cleanup
    std::fs::remove_file(&db_path).ok();
}

#[test]
fn test_compute_hash() {
    let hash1 = compute_hash("content1");
    let hash2 = compute_hash("content1");
    let hash3 = compute_hash("content2");

    // Same content produces same hash
    assert_eq!(hash1, hash2);

    // Different content produces different hash
    assert_ne!(hash1, hash3);

    // Hash is hexadecimal
    assert!(hash1.chars().all(|c| c.is_ascii_hexdigit()));
}
