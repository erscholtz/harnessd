//! Integration tests for daemon lock functionality.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use harnessd::daemon_lock::{DaemonLock, read_daemon_pid};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_runtime_dir() -> PathBuf {
    let temp_dir = std::env::temp_dir();
    let unique_name = format!(
        "harnessd_lock_test_{}_{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    temp_dir.join(unique_name)
}

#[test]
fn test_daemon_lock_acquire_and_release() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    // Acquire lock
    let lock = DaemonLock::acquire(&runtime_dir);
    assert!(lock.is_ok(), "failed to acquire lock: {:?}", lock.err());

    // Verify lock file exists
    let lock_path = runtime_dir.join("daemon.lock");
    assert!(lock_path.exists());

    // Verify PID is written
    let pid = read_daemon_pid(&runtime_dir);
    assert!(pid.is_ok());
    assert_eq!(pid.unwrap(), std::process::id());

    // Drop lock
    drop(lock);

    // Verify lock file is removed
    assert!(!lock_path.exists());

    // Cleanup
    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[test]
fn test_daemon_lock_prevents_double_acquire() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    // Acquire first lock
    let _lock1 = DaemonLock::acquire(&runtime_dir).expect("failed to acquire first lock");

    // Try to acquire second lock (should fail)
    let lock2 = DaemonLock::acquire(&runtime_dir);
    assert!(lock2.is_err());

    if let Err(e) = lock2 {
        let err_msg = e.to_string();
        assert!(err_msg.contains("another daemon instance may be running"));
    }

    // Cleanup
    drop(_lock1);
    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[test]
fn test_daemon_lock_released_on_drop() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    // Acquire and release lock
    {
        let _lock = DaemonLock::acquire(&runtime_dir).expect("failed to acquire lock");
    }

    // Should be able to acquire again
    let lock2 = DaemonLock::acquire(&runtime_dir);
    assert!(lock2.is_ok(), "lock should be released after drop");

    // Cleanup
    drop(lock2);
    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[test]
fn test_read_daemon_pid_no_lock() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    // Try to read PID without lock file
    let result = read_daemon_pid(&runtime_dir);
    assert!(result.is_err());

    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("no daemon lock"));

    // Cleanup
    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[test]
fn test_daemon_lock_creates_runtime_dir() {
    let runtime_dir = temp_runtime_dir().join("nested").join("dirs");

    // Runtime dir doesn't exist yet
    assert!(!runtime_dir.exists());

    // Acquire lock should create it
    let lock = DaemonLock::acquire(&runtime_dir);
    assert!(lock.is_ok());
    assert!(runtime_dir.exists());

    // Cleanup
    drop(lock);
    std::fs::remove_dir_all(&runtime_dir).ok();
}
