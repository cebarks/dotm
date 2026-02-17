use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn hash_file(path: &Path) -> Result<String> {
    let content = std::fs::read(path)
        .with_context(|| format!("failed to read file for hashing: {}", path.display()))?;
    Ok(hash_content(&content))
}

pub fn hash_content(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn hash_file_returns_consistent_sha256() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "hello world").unwrap();

        let hash1 = hash_file(&path).unwrap();
        let hash2 = hash_file(&path).unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn hash_content_matches_hash_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        let content = "some content";
        std::fs::write(&path, content).unwrap();

        assert_eq!(hash_file(&path).unwrap(), hash_content(content.as_bytes()));
    }
}
