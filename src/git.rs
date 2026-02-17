use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DirtyFile {
    pub path: String,
    pub status: DirtyStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DirtyStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
}

#[derive(Debug)]
pub enum PushResult {
    Success,
    NoRemote,
    Rejected(String),
    Error(String),
}

#[derive(Debug)]
pub enum PullResult {
    Success,
    NoRemote,
    AlreadyUpToDate,
    Conflicts(Vec<String>),
    Error(String),
}

#[derive(Debug)]
pub struct GitSummary {
    pub branch: Option<String>,
    pub dirty_count: usize,
    pub untracked_count: usize,
    pub modified_count: usize,
    pub ahead_behind: Option<(usize, usize)>,
}

pub struct GitRepo {
    repo: gix::Repository,
}

impl GitRepo {
    /// Attempt to open (discover) a git repository at or above `path`.
    /// Returns `None` if `path` is not inside a git repository.
    pub fn open(path: &Path) -> Option<Self> {
        let repo = gix::discover(path).ok()?;
        Some(Self { repo })
    }

    /// Returns the current branch name, or `None` if HEAD is detached.
    pub fn branch_name(&self) -> Result<Option<String>> {
        let head = self.repo.head()?;
        let name = head
            .referent_name()
            .map(|full| full.shorten().to_string());
        Ok(name)
    }

    /// Returns a high-level summary of the repository state: branch, dirty counts, ahead/behind.
    pub fn summary(&self) -> Result<GitSummary> {
        let branch = self.branch_name()?;
        let dirty = self.dirty_files()?;

        let untracked_count = dirty
            .iter()
            .filter(|f| matches!(f.status, DirtyStatus::Untracked))
            .count();
        let modified_count = dirty
            .iter()
            .filter(|f| !matches!(f.status, DirtyStatus::Untracked))
            .count();

        let ahead_behind = self.ahead_behind()?;

        Ok(GitSummary {
            branch,
            dirty_count: dirty.len(),
            untracked_count,
            modified_count,
            ahead_behind,
        })
    }

    /// Returns true if the working tree has any uncommitted changes or untracked files.
    pub fn is_dirty(&self) -> Result<bool> {
        let files = self.dirty_files()?;
        Ok(!files.is_empty())
    }

    /// Returns (ahead, behind) counts relative to the upstream tracking branch.
    /// Returns None if there's no tracking branch configured or HEAD is detached.
    pub fn ahead_behind(&self) -> Result<Option<(usize, usize)>> {
        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| anyhow::anyhow!("bare repository has no working directory"))?;

        let output = std::process::Command::new("git")
            .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
            .current_dir(workdir)
            .output()?;

        if !output.status.success() {
            // No upstream configured, detached HEAD, etc.
            return Ok(None);
        }

        let stdout = String::from_utf8(output.stdout)?;
        let parts: Vec<&str> = stdout.trim().split('\t').collect();
        if parts.len() != 2 {
            return Ok(None);
        }

        let ahead = parts[0].parse::<usize>().unwrap_or(0);
        let behind = parts[1].parse::<usize>().unwrap_or(0);

        Ok(Some((ahead, behind)))
    }

    /// Stage all changes and create a commit. Errors if there's nothing to commit.
    pub fn commit_all(&self, message: &str) -> Result<()> {
        if !self.is_dirty()? {
            anyhow::bail!("nothing to commit — working tree is clean");
        }

        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| anyhow::anyhow!("bare repository has no working directory"))?;

        let status = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(workdir)
            .status()?;

        if !status.success() {
            anyhow::bail!("git add failed with exit code {}", status);
        }

        let status = std::process::Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(workdir)
            .status()?;

        if !status.success() {
            anyhow::bail!("git commit failed with exit code {}", status);
        }

        Ok(())
    }

    /// Returns a list of dirty files with their statuses.
    /// Uses `git status --porcelain` for reliable results across all repo states.
    pub fn dirty_files(&self) -> Result<Vec<DirtyFile>> {
        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| anyhow::anyhow!("bare repository has no working directory"))?;

        let output = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(workdir)
            .output()?;

        anyhow::ensure!(
            output.status.success(),
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8(output.stdout)?;
        let mut files = Vec::new();

        for line in stdout.lines() {
            if line.len() < 4 {
                continue;
            }
            let index_status = line.as_bytes()[0];
            let worktree_status = line.as_bytes()[1];
            let path = line[3..].to_string();

            let status = match (index_status, worktree_status) {
                (b'?', b'?') => DirtyStatus::Untracked,
                (b'A', _) | (_, b'A') => DirtyStatus::Added,
                (b'D', _) | (_, b'D') => DirtyStatus::Deleted,
                _ => DirtyStatus::Modified,
            };

            files.push(DirtyFile { path, status });
        }

        Ok(files)
    }

    fn has_remote(&self) -> bool {
        self.repo.remote_names().first().is_some()
    }

    pub fn push(&self) -> Result<PushResult> {
        if !self.has_remote() {
            return Ok(PushResult::NoRemote);
        }

        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| anyhow::anyhow!("bare repository has no working directory"))?;

        let output = std::process::Command::new("git")
            .args(["push"])
            .current_dir(workdir)
            .output()?;

        if output.status.success() {
            Ok(PushResult::Success)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if stderr.contains("rejected") {
                Ok(PushResult::Rejected(stderr))
            } else {
                Ok(PushResult::Error(stderr))
            }
        }
    }

    pub fn pull(&self) -> Result<PullResult> {
        if !self.has_remote() {
            return Ok(PullResult::NoRemote);
        }

        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| anyhow::anyhow!("bare repository has no working directory"))?;

        let output = std::process::Command::new("git")
            .args(["pull"])
            .current_dir(workdir)
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("Already up to date") {
                Ok(PullResult::AlreadyUpToDate)
            } else {
                Ok(PullResult::Success)
            }
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stdout.contains("CONFLICT") || stderr.contains("CONFLICT") {
                let conflicts = self.list_conflicted_files()?;
                Ok(PullResult::Conflicts(conflicts))
            } else {
                Ok(PullResult::Error(stderr.to_string()))
            }
        }
    }

    fn list_conflicted_files(&self) -> Result<Vec<String>> {
        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| anyhow::anyhow!("bare repository has no working directory"))?;

        let output = std::process::Command::new("git")
            .args(["diff", "--name-only", "--diff-filter=U"])
            .current_dir(workdir)
            .output()?;

        let files = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect();

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_returns_none_for_non_repo() {
        let dir = TempDir::new().unwrap();
        assert!(GitRepo::open(dir.path()).is_none());
    }

    #[test]
    fn open_returns_some_for_git_repo() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        assert!(GitRepo::open(dir.path()).is_some());
    }

    #[test]
    fn branch_name_on_fresh_repo() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        let name = repo.branch_name().unwrap();
        assert_eq!(name, Some("main".to_string()));
    }

    #[test]
    fn is_dirty_on_clean_repo() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        assert!(!repo.is_dirty().unwrap());
    }

    #[test]
    fn is_dirty_with_untracked_file() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hello").unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        assert!(repo.is_dirty().unwrap());
    }

    #[test]
    fn dirty_files_lists_changes() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("a.txt"), "aaa").unwrap();
        std::fs::write(dir.path().join("b.txt"), "bbb").unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        let files = repo.dirty_files().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.status == DirtyStatus::Untracked));
    }

    #[test]
    fn ahead_behind_returns_none_without_remote() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        let result = repo.ahead_behind().unwrap();
        assert_eq!(result, None);
    }

    /// Configure a minimal git identity in the given repo so `git commit` works.
    fn configure_test_identity(dir: &Path) {
        for (key, value) in [
            ("user.name", "Test User"),
            ("user.email", "test@test.com"),
        ] {
            std::process::Command::new("git")
                .args(["config", key, value])
                .current_dir(dir)
                .status()
                .unwrap();
        }
    }

    #[test]
    fn commit_all_creates_commit() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        configure_test_identity(dir.path());
        std::fs::write(dir.path().join("file.txt"), "content").unwrap();

        let repo = GitRepo::open(dir.path()).unwrap();
        repo.commit_all("test commit").unwrap();

        let gix_repo = gix::open(dir.path()).unwrap();
        let head = gix_repo.head_commit().unwrap();
        let msg = head.message_raw_sloppy();
        assert!(
            msg.starts_with(b"test commit"),
            "commit message should match"
        );
    }

    #[test]
    fn commit_all_errors_when_nothing_to_commit() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        let result = repo.commit_all("empty commit");
        assert!(result.is_err());
    }

    #[test]
    fn push_returns_no_remote_without_remote() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        let result = repo.push().unwrap();
        assert!(matches!(result, PushResult::NoRemote));
    }

    #[test]
    fn pull_returns_no_remote_without_remote() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        let result = repo.pull().unwrap();
        assert!(matches!(result, PullResult::NoRemote));
    }

    #[test]
    fn summary_clean_repo() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        let summary = repo.summary().unwrap();
        assert!(summary.branch.is_some());
        assert_eq!(summary.dirty_count, 0);
        assert!(summary.ahead_behind.is_none());
    }

    #[test]
    fn summary_with_dirty_files() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("file.txt"), "content").unwrap();
        let repo = GitRepo::open(dir.path()).unwrap();
        let summary = repo.summary().unwrap();
        assert!(summary.dirty_count > 0);
    }
}
