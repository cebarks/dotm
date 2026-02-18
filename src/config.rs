use serde::Deserialize;
use std::collections::HashMap;
use toml::map::Map;
use toml::Value;

#[derive(Debug, Deserialize)]
pub struct RootConfig {
    pub dotm: DotmSettings,
    #[serde(default)]
    pub packages: HashMap<String, PackageConfig>,
}

#[derive(Debug, Deserialize)]
pub struct DotmSettings {
    pub target: String,
    #[serde(default = "default_packages_dir")]
    pub packages_dir: String,
    #[serde(default)]
    pub auto_prune: bool,
}

fn default_packages_dir() -> String {
    "packages".to_string()
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum DeployStrategy {
    Stage,
    Copy,
}

#[derive(Debug, Default, Deserialize)]
pub struct PackageConfig {
    pub description: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub suggests: Vec<String>,
    pub target: Option<String>,
    pub strategy: Option<DeployStrategy>,
    #[serde(default)]
    pub permissions: HashMap<String, String>,
    #[serde(default)]
    pub system: bool,
    pub owner: Option<String>,
    pub group: Option<String>,
    #[serde(default)]
    pub ownership: HashMap<String, String>,
    #[serde(default)]
    pub preserve: HashMap<String, Vec<String>>,
}

pub fn validate_system_packages(root: &RootConfig) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, pkg) in &root.packages {
        if pkg.system {
            if pkg.target.is_none() {
                errors.push(format!(
                    "system package '{name}' must specify a target directory"
                ));
            }
            if pkg.strategy.is_none() {
                errors.push(format!(
                    "system package '{name}' must specify a deployment strategy"
                ));
            }
        }
        // Validate ownership format
        for (path, value) in &pkg.ownership {
            if value.split(':').count() != 2 {
                errors.push(format!(
                    "package '{name}': invalid ownership format for '{path}': expected 'user:group', got '{value}'"
                ));
            }
        }
        // Validate permissions format
        for (path, value) in &pkg.permissions {
            if u32::from_str_radix(value, 8).is_err() {
                errors.push(format!(
                    "package '{name}': invalid permission for '{path}': '{value}' is not valid octal"
                ));
            }
        }
        // Validate preserve entries don't conflict
        for (path, preserve_fields) in &pkg.preserve {
            for field in preserve_fields {
                match field.as_str() {
                    "owner" | "group" => {
                        if pkg.ownership.contains_key(path) {
                            errors.push(format!(
                                "package '{name}': file '{path}' has both preserve {field} and ownership override"
                            ));
                        }
                    }
                    "mode" => {
                        if pkg.permissions.contains_key(path) {
                            errors.push(format!(
                                "package '{name}': file '{path}' has both preserve mode and permission override"
                            ));
                        }
                    }
                    other => {
                        errors.push(format!(
                            "package '{name}': file '{path}': unknown preserve field '{other}'"
                        ));
                    }
                }
            }
        }
    }
    errors
}

#[derive(Debug, Deserialize)]
pub struct HostConfig {
    pub hostname: String,
    pub roles: Vec<String>,
    #[serde(default)]
    pub vars: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct RoleConfig {
    pub packages: Vec<String>,
    #[serde(default)]
    pub vars: Map<String, Value>,
}
