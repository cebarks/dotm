use crate::hash;
use anyhow::Result;
use std::path::Path;

/// Compute the content hash for a package's setup command, used for
/// change detection. If `setup` looks like a script path (no whitespace,
/// and resolves to an existing file under the package directory), hash
/// the file's bytes. Otherwise hash the command string itself.
pub fn compute_setup_hash(pkg_dir: &Path, setup_cmd: &str) -> Result<String> {
    if !setup_cmd.contains(char::is_whitespace) {
        let script_path = pkg_dir.join(setup_cmd);
        if script_path.is_file() {
            return hash::hash_file(&script_path);
        }
    }
    Ok(hash::hash_content(setup_cmd.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn inline_command_hashes_the_string() {
        let dir = TempDir::new().unwrap();
        let h1 = compute_setup_hash(dir.path(), "brew bundle --file=~/.Brewfile").unwrap();
        let h2 = compute_setup_hash(dir.path(), "brew bundle --file=~/.Brewfile").unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1, hash::hash_content(b"brew bundle --file=~/.Brewfile"));
    }

    #[test]
    fn existing_script_path_hashes_file_contents() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
        let script = dir.path().join("scripts/apply.sh");
        std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();

        let h = compute_setup_hash(dir.path(), "scripts/apply.sh").unwrap();
        assert_eq!(h, hash::hash_file(&script).unwrap());
    }

    #[test]
    fn missing_script_path_falls_back_to_string_hash() {
        let dir = TempDir::new().unwrap();
        let h = compute_setup_hash(dir.path(), "scripts/missing.sh").unwrap();
        assert_eq!(h, hash::hash_content(b"scripts/missing.sh"));
    }

    #[test]
    fn script_hash_changes_when_file_content_changes() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("setup.sh");
        std::fs::write(&script, "echo one").unwrap();
        let h1 = compute_setup_hash(dir.path(), "setup.sh").unwrap();

        std::fs::write(&script, "echo two").unwrap();
        let h2 = compute_setup_hash(dir.path(), "setup.sh").unwrap();

        assert_ne!(h1, h2);
    }

    #[test]
    fn multiword_command_never_treated_as_script_even_if_first_word_is_a_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("setup.sh"), "echo hi").unwrap();
        let h = compute_setup_hash(dir.path(), "setup.sh --flag").unwrap();
        assert_eq!(h, hash::hash_content(b"setup.sh --flag"));
    }
}
