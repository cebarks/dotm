use dotm::hooks::run_hook;
use tempfile::TempDir;

#[test]
fn run_hook_success() {
    let dir = TempDir::new().unwrap();
    let result = run_hook("true", dir.path(), "test-pkg", "deploy");
    assert!(result.is_ok());
}

#[test]
fn run_hook_failure_returns_error() {
    let dir = TempDir::new().unwrap();
    let result = run_hook("false", dir.path(), "test-pkg", "deploy");
    assert!(result.is_err());
}

#[test]
fn run_hook_sets_env_vars() {
    let dir = TempDir::new().unwrap();
    let out_file = dir.path().join("env_out");
    let cmd = format!(
        "echo $DOTM_PACKAGE,$DOTM_TARGET,$DOTM_ACTION > {}",
        out_file.display()
    );
    run_hook(&cmd, dir.path(), "mypkg", "deploy").unwrap();
    let content = std::fs::read_to_string(&out_file).unwrap();
    assert!(content.contains("mypkg"));
    assert!(content.contains("deploy"));
}

#[test]
fn empty_hook_is_noop() {
    let dir = TempDir::new().unwrap();
    let result = run_hook("", dir.path(), "test-pkg", "deploy");
    assert!(result.is_ok());
}
