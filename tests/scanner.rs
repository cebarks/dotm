use dotm::scanner::{self, scan_package};
use std::path::Path;

#[test]
fn scan_resolves_host_override_over_base() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    let app_conf = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(
        app_conf
            .source
            .to_str()
            .unwrap()
            .ends_with("app.conf##host.myhost")
    );
    assert_eq!(
        app_conf.kind,
        dotm::scanner::EntryKind::Override,
        "overrides should be copied, not symlinked"
    );
}

#[test]
fn scan_resolves_role_override_when_no_host_override() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "anotherhost", &["desktop"]).unwrap();

    let app_conf = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(
        app_conf
            .source
            .to_str()
            .unwrap()
            .ends_with("app.conf##role.desktop")
    );
    assert_eq!(app_conf.kind, dotm::scanner::EntryKind::Override);
}

#[test]
fn scan_uses_base_when_no_overrides_match() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "anotherhost", &["server"]).unwrap();

    let app_conf = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(app_conf.source.to_str().unwrap().ends_with("app.conf"));
    assert!(!app_conf.source.to_str().unwrap().contains("##"));
    assert_eq!(
        app_conf.kind,
        dotm::scanner::EntryKind::Base,
        "plain files should be symlinked"
    );
}

#[test]
fn scan_plain_file_is_symlinked() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    let profile = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".profile"))
        .unwrap();
    assert_eq!(profile.kind, dotm::scanner::EntryKind::Base);
}

#[test]
fn scan_template_is_detected() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    let tmpl = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/templated.conf"))
        .unwrap();
    assert_eq!(tmpl.kind, dotm::scanner::EntryKind::Template);
    assert!(tmpl.source.to_str().unwrap().ends_with(".tera"));
}

#[test]
fn scan_excludes_non_matching_overrides() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    assert!(
        actions
            .iter()
            .all(|a| !a.source.to_str().unwrap().contains("##host.other"))
    );
    let app_count = actions
        .iter()
        .filter(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .count();
    assert_eq!(app_count, 1);
}

#[test]
fn scan_theme_conf_has_no_override() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    let theme = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/theme.conf"))
        .unwrap();
    assert_eq!(theme.kind, dotm::scanner::EntryKind::Base);
}

// --- Substring matching tests (fix #1) ---

#[test]
fn scan_host_override_does_not_substring_match() {
    let pkg_dir = Path::new("tests/fixtures/substring_overrides");
    let actions = scan_package(pkg_dir, "dev", &[]).unwrap();

    let app = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(
        app.source.to_str().unwrap().ends_with("app.conf##host.dev"),
        "host 'dev' should match ##host.dev exactly, not ##host.dev-staging, got: {}",
        app.source.display()
    );
}

#[test]
fn scan_host_override_exact_match_longer_hostname() {
    let pkg_dir = Path::new("tests/fixtures/substring_overrides");
    let actions = scan_package(pkg_dir, "dev-staging", &[]).unwrap();

    let app = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(
        app.source
            .to_str()
            .unwrap()
            .ends_with("app.conf##host.dev-staging"),
        "host 'dev-staging' should match ##host.dev-staging exactly, got: {}",
        app.source.display()
    );
}

#[test]
fn scan_role_override_does_not_substring_match() {
    let pkg_dir = Path::new("tests/fixtures/substring_overrides");
    let actions = scan_package(pkg_dir, "other", &["desktop"]).unwrap();

    let app = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(
        app.source
            .to_str()
            .unwrap()
            .ends_with("app.conf##role.desktop"),
        "role 'desktop' should match ##role.desktop exactly, not ##role.desktop-gaming, got: {}",
        app.source.display()
    );
}

#[test]
fn scan_no_host_match_falls_through_to_base() {
    let pkg_dir = Path::new("tests/fixtures/substring_overrides");
    let actions = scan_package(pkg_dir, "unrelated", &[]).unwrap();

    let app = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(
        !app.source.to_str().unwrap().contains("##"),
        "unrelated host with no roles should fall through to base, got: {}",
        app.source.display()
    );
    assert_eq!(app.kind, scanner::EntryKind::Base);
}

// --- Template override tests (fix #2) ---

#[test]
fn scan_host_tera_override_classified_as_template() {
    let pkg_dir = Path::new("tests/fixtures/substring_overrides");
    let actions = scan_package(pkg_dir, "dev", &[]).unwrap();

    let rendered = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/rendered.conf"))
        .unwrap();
    assert!(
        rendered
            .source
            .to_str()
            .unwrap()
            .ends_with("rendered.conf##host.dev.tera"),
        "should select the host .tera override, got: {}",
        rendered.source.display()
    );
    assert_eq!(
        rendered.kind,
        scanner::EntryKind::Template,
        ".tera host override should be classified as Template for rendering"
    );
}

#[test]
fn scan_role_tera_override_classified_as_template() {
    let pkg_dir = Path::new("tests/fixtures/substring_overrides");
    let actions = scan_package(pkg_dir, "other", &["desktop"]).unwrap();

    let rendered = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/rendered.conf"))
        .unwrap();
    assert!(
        rendered
            .source
            .to_str()
            .unwrap()
            .ends_with("rendered.conf##role.desktop.tera"),
        "should select the role .tera override, got: {}",
        rendered.source.display()
    );
    assert_eq!(
        rendered.kind,
        scanner::EntryKind::Template,
        ".tera role override should be classified as Template for rendering"
    );
}

#[test]
fn scan_tera_override_has_priority_over_base() {
    let pkg_dir = Path::new("tests/fixtures/substring_overrides");
    let actions = scan_package(pkg_dir, "dev", &["desktop"]).unwrap();

    let rendered = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/rendered.conf"))
        .unwrap();
    // Host override takes priority over role override
    assert!(
        rendered
            .source
            .to_str()
            .unwrap()
            .ends_with("rendered.conf##host.dev.tera"),
        "host .tera override should take priority over role .tera override, got: {}",
        rendered.source.display()
    );
}
