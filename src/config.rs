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
}

fn default_packages_dir() -> String {
    "packages".to_string()
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeployStrategy {
    #[default]
    Stage,
    Copy,
}

#[derive(Debug, Deserialize)]
pub struct PackageConfig {
    pub description: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub suggests: Vec<String>,
    pub target: Option<String>,
    #[serde(default)]
    pub strategy: DeployStrategy,
    #[serde(default)]
    pub permissions: HashMap<String, String>,
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
