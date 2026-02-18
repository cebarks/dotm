#[test]
fn expands_home_env_var() {
    // SAFETY: test runs in isolation, no concurrent env access
    unsafe { std::env::set_var("HOME", "/test/home") };
    let result = dotm::orchestrator::expand_path("$HOME/.config", None).unwrap();
    assert_eq!(result, "/test/home/.config");
}

#[test]
fn expands_tilde() {
    let result = dotm::orchestrator::expand_path("~/.config", None).unwrap();
    assert!(result.starts_with('/'));
    assert!(result.ends_with("/.config"));
}

#[test]
fn errors_on_undefined_var() {
    // SAFETY: test runs in isolation, no concurrent env access
    unsafe { std::env::remove_var("NONEXISTENT_DOTM_TEST_VAR") };
    let result = dotm::orchestrator::expand_path("$NONEXISTENT_DOTM_TEST_VAR/foo", None);
    assert!(result.is_err());
}

#[test]
fn expands_xdg_config_home() {
    // SAFETY: test runs in isolation, no concurrent env access
    unsafe { std::env::set_var("XDG_CONFIG_HOME", "/custom/config") };
    let result = dotm::orchestrator::expand_path("$XDG_CONFIG_HOME/app", None).unwrap();
    assert_eq!(result, "/custom/config/app");
}

#[test]
fn expand_path_with_context() {
    // SAFETY: test runs in isolation, no concurrent env access
    unsafe { std::env::remove_var("NONEXISTENT_DOTM_TEST_VAR2") };
    let result = dotm::orchestrator::expand_path("$NONEXISTENT_DOTM_TEST_VAR2/foo", Some("package 'shell'"));
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("package 'shell'"));
}
