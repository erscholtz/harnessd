//! Unit tests for paths module.

use harnessd::paths::runtime_dir;

#[test]
fn test_runtime_dir_returns_valid_path() {
    let dir = runtime_dir();

    // Should not be empty
    assert!(!dir.as_os_str().is_empty());

    // Should end with "harnessd"
    assert!(dir.to_string_lossy().ends_with("harnessd"));
}

#[test]
fn test_runtime_dir_is_absolute() {
    let dir = runtime_dir();
    assert!(dir.is_absolute());
}

#[cfg(windows)]
#[test]
fn test_runtime_dir_on_windows() {
    let dir = runtime_dir();
    let path_str = dir.to_string_lossy();

    // On Windows, should be under LOCALAPPDATA
    assert!(
        path_str.contains("AppData") || path_str.contains("Application Data"),
        "Windows runtime dir should be under AppData: {}",
        path_str
    );
}

#[cfg(unix)]
#[test]
fn test_runtime_dir_on_unix() {
    let dir = runtime_dir();
    let path_str = dir.to_string_lossy();

    // On Unix, should be under .local/share
    assert!(
        path_str.contains(".local/share"),
        "Unix runtime dir should be under .local/share: {}",
        path_str
    );
}
