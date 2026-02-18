use dotm::config::{validate_system_packages, HostConfig, RoleConfig, RootConfig};

#[test]
fn parse_minimal_root_config() {
    let toml_str = r#"
[dotm]
target = "~"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.dotm.target, "~");
    assert_eq!(config.dotm.packages_dir, "packages"); // default
    assert!(config.packages.is_empty());
}

#[test]
fn parse_root_config_with_packages() {
    let toml_str = r#"
[dotm]
target = "~"
packages_dir = "pkgs"

[packages.zsh]
description = "Zsh shell configuration"

[packages.kde]
description = "KDE Plasma desktop configs"
depends = ["util"]
suggests = ["gaming"]

[packages.util]
description = "General utility configs"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.dotm.packages_dir, "pkgs");
    assert_eq!(config.packages.len(), 3);

    let kde = &config.packages["kde"];
    assert_eq!(kde.description.as_deref(), Some("KDE Plasma desktop configs"));
    assert_eq!(kde.depends, vec!["util"]);
    assert_eq!(kde.suggests, vec!["gaming"]);

    let zsh = &config.packages["zsh"];
    assert!(zsh.depends.is_empty());
    assert!(zsh.suggests.is_empty());
}

#[test]
fn parse_root_config_with_package_target_override() {
    let toml_str = r#"
[dotm]
target = "~"

[packages.system]
description = "System-level configs"
target = "/"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    let system = &config.packages["system"];
    assert_eq!(system.target.as_deref(), Some("/"));
}

#[test]
fn parse_host_config() {
    let toml_str = r#"
hostname = "testhost"
roles = ["desktop", "gaming", "dev"]

[vars]
display.resolution = "3840x2160"
display.refresh_rate = 120
gpu.vendor = "amd"
"#;
    let config: HostConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.hostname, "testhost");
    assert_eq!(config.roles, vec!["desktop", "gaming", "dev"]);

    let display = config.vars.get("display").unwrap().as_table().unwrap();
    assert_eq!(display.get("resolution").unwrap().as_str().unwrap(), "3840x2160");
    assert_eq!(display.get("refresh_rate").unwrap().as_integer().unwrap(), 120);
}

#[test]
fn parse_host_config_no_vars() {
    let toml_str = r#"
hostname = "minimal"
roles = ["dev"]
"#;
    let config: HostConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.hostname, "minimal");
    assert!(config.vars.is_empty());
}

#[test]
fn parse_role_config() {
    let toml_str = r#"
packages = ["games", "gamemode"]

[vars]
gamemode.renice = 10
"#;
    let config: RoleConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.packages, vec!["games", "gamemode"]);
    let gamemode = config.vars.get("gamemode").unwrap().as_table().unwrap();
    assert_eq!(gamemode.get("renice").unwrap().as_integer().unwrap(), 10);
}

#[test]
fn parse_role_config_no_vars() {
    let toml_str = r#"
packages = ["zsh", "ssh"]
"#;
    let config: RoleConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.packages, vec!["zsh", "ssh"]);
    assert!(config.vars.is_empty());
}

#[test]
fn parse_package_with_strategy_and_permissions() {
    let toml_str = r#"
[dotm]
target = "~"

[packages.system]
description = "System configs"
target = "/"
strategy = "copy"

[packages.bin]
description = "Scripts"

[packages.bin.permissions]
"bin/myscript" = "755"
"bin/helper" = "700"
"#;

    let config: RootConfig = toml::from_str(toml_str).unwrap();

    let system = &config.packages["system"];
    assert_eq!(system.strategy, Some(dotm::config::DeployStrategy::Copy));

    let bin = &config.packages["bin"];
    assert_eq!(bin.strategy, None);
    let perms = &bin.permissions;
    assert_eq!(perms.get("bin/myscript").unwrap(), "755");
    assert_eq!(perms.get("bin/helper").unwrap(), "700");
}

#[test]
fn parse_system_package_with_ownership() {
    let toml_str = r#"
[dotm]
target = "/"

[packages.etc]
description = "System configuration files"
target = "/"
strategy = "copy"
system = true
owner = "root"
group = "root"

[packages.etc.ownership]
"etc/shadow" = "root:shadow"
"etc/gshadow" = "root:shadow"

[packages.etc.preserve]
"etc/shadow" = ["mode", "ownership"]
"etc/gshadow" = ["mode"]

[packages.etc.permissions]
"etc/shadow" = "640"
"#;

    let config: RootConfig = toml::from_str(toml_str).unwrap();
    let etc = &config.packages["etc"];

    assert!(etc.system);
    assert_eq!(etc.owner.as_deref(), Some("root"));
    assert_eq!(etc.group.as_deref(), Some("root"));
    assert_eq!(etc.strategy, Some(dotm::config::DeployStrategy::Copy));

    assert_eq!(etc.ownership.get("etc/shadow").unwrap(), "root:shadow");
    assert_eq!(etc.ownership.get("etc/gshadow").unwrap(), "root:shadow");

    let shadow_preserve = etc.preserve.get("etc/shadow").unwrap();
    assert_eq!(shadow_preserve, &vec!["mode".to_string(), "ownership".to_string()]);

    let gshadow_preserve = etc.preserve.get("etc/gshadow").unwrap();
    assert_eq!(gshadow_preserve, &vec!["mode".to_string()]);

    assert_eq!(etc.permissions.get("etc/shadow").unwrap(), "640");
}

#[test]
fn parse_non_system_package_defaults() {
    let toml_str = r#"
[dotm]
target = "~"

[packages.zsh]
description = "Zsh shell configuration"
"#;

    let config: RootConfig = toml::from_str(toml_str).unwrap();
    let zsh = &config.packages["zsh"];

    assert!(!zsh.system);
    assert!(zsh.owner.is_none());
    assert!(zsh.group.is_none());
    assert!(zsh.ownership.is_empty());
    assert!(zsh.preserve.is_empty());
}

#[test]
fn validate_system_package_missing_target() {
    let toml_str = r#"
[dotm]
target = "~"
[packages.bad]
system = true
strategy = "copy"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    let errors = validate_system_packages(&config);
    assert!(errors.iter().any(|e| e.contains("must specify a target")));
}

#[test]
fn validate_system_package_missing_strategy() {
    let toml_str = r#"
[dotm]
target = "~"
[packages.bad]
system = true
target = "/etc/foo"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    let errors = validate_system_packages(&config);
    assert!(errors
        .iter()
        .any(|e| e.contains("must specify a deployment strategy")));
}

#[test]
fn validate_invalid_ownership_format() {
    let toml_str = r#"
[dotm]
target = "~"
[packages.bad]
[packages.bad.ownership]
"file.conf" = "justuser"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    let errors = validate_system_packages(&config);
    assert!(errors.iter().any(|e| e.contains("invalid ownership format")));
}

#[test]
fn validate_preserve_conflicts_with_ownership() {
    let toml_str = r#"
[dotm]
target = "~"
[packages.bad]
[packages.bad.ownership]
"file.conf" = "root:root"
[packages.bad.preserve]
"file.conf" = ["owner"]
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    let errors = validate_system_packages(&config);
    assert!(errors
        .iter()
        .any(|e| e.contains("preserve") && e.contains("ownership")));
}

#[test]
fn validate_valid_system_package_no_errors() {
    let toml_str = r#"
[dotm]
target = "~"
[packages.good]
system = true
target = "/etc/foo"
strategy = "copy"
owner = "root"
group = "root"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    let errors = validate_system_packages(&config);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}
