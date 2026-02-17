use crate::state::{DeployEntry, FileStatus};
use crossterm::style::Stylize;
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::Path;

pub struct PackageStatus {
    pub name: String,
    pub total: usize,
    pub ok: usize,
    pub modified: usize,
    pub missing: usize,
    pub files: Vec<FileEntry>,
}

pub struct FileEntry {
    pub display_path: String,
    pub status: FileStatus,
}

pub fn group_by_package(entries: &[DeployEntry], statuses: &[FileStatus]) -> Vec<PackageStatus> {
    let mut groups: BTreeMap<&str, Vec<(String, FileStatus)>> = BTreeMap::new();

    for (entry, status) in entries.iter().zip(statuses.iter()) {
        groups
            .entry(&entry.package)
            .or_default()
            .push((display_path(&entry.target), status.clone()));
    }

    groups
        .into_iter()
        .map(|(name, files)| {
            let total = files.len();
            let ok = files.iter().filter(|(_, s)| *s == FileStatus::Ok).count();
            let modified = files
                .iter()
                .filter(|(_, s)| *s == FileStatus::Modified)
                .count();
            let missing = files
                .iter()
                .filter(|(_, s)| *s == FileStatus::Missing)
                .count();
            let file_entries = files
                .into_iter()
                .map(|(display_path, status)| FileEntry {
                    display_path,
                    status,
                })
                .collect();

            PackageStatus {
                name: name.to_string(),
                total,
                ok,
                modified,
                missing,
                files: file_entries,
            }
        })
        .collect()
}

fn display_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let home = Path::new(&home);
        if let Ok(rest) = path.strip_prefix(home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

pub fn render_default(groups: &[PackageStatus]) -> String {
    let mut out = String::new();

    for pkg in groups {
        out.push_str(&format!(
            "{} ({}, {})\n",
            pkg.name,
            files_label(pkg.total),
            status_summary(pkg),
        ));

        for file in &pkg.files {
            match file.status {
                FileStatus::Ok => {}
                FileStatus::Modified => {
                    out.push_str(&format!("  M {}\n", file.display_path));
                }
                FileStatus::Missing => {
                    out.push_str(&format!("  ! {}\n", file.display_path));
                }
            }
        }
    }

    out
}

pub fn render_verbose(groups: &[PackageStatus]) -> String {
    let mut out = String::new();

    for pkg in groups {
        out.push_str(&format!(
            "{} ({}, {})\n",
            pkg.name,
            files_label(pkg.total),
            status_summary(pkg),
        ));

        for file in &pkg.files {
            let marker = match file.status {
                FileStatus::Ok => "~",
                FileStatus::Modified => "M",
                FileStatus::Missing => "!",
            };
            out.push_str(&format!("  {} {}\n", marker, file.display_path));
        }
    }

    out
}

pub fn render_short(total: usize, modified: usize, missing: usize) -> String {
    let _ = total;
    if modified == 0 && missing == 0 {
        return String::new();
    }

    let mut parts = Vec::new();
    if modified > 0 {
        parts.push(format!("{modified} modified"));
    }
    if missing > 0 {
        parts.push(format!("{missing} missing"));
    }
    format!("dotm: {}\n", parts.join(", "))
}

pub fn render_footer(total: usize, modified: usize, missing: usize) -> String {
    if modified == 0 && missing == 0 {
        return format!("{total} managed, all ok.\n");
    }

    let mut parts = vec![format!("{total} managed")];
    if modified > 0 {
        parts.push(format!("{modified} modified"));
    }
    if missing > 0 {
        parts.push(format!("{missing} missing"));
    }
    format!("{}.\n", parts.join(", "))
}

fn files_label(count: usize) -> String {
    if count == 1 {
        "1 file".to_string()
    } else {
        format!("{count} files")
    }
}

fn status_summary(pkg: &PackageStatus) -> String {
    if pkg.modified == 0 && pkg.missing == 0 {
        return "ok".to_string();
    }

    let mut parts = Vec::new();
    if pkg.modified > 0 {
        parts.push(format!("{} modified", pkg.modified));
    }
    if pkg.missing > 0 {
        parts.push(format!("{} missing", pkg.missing));
    }
    parts.join(", ")
}

pub fn use_color() -> bool {
    std::env::var("NO_COLOR").is_err() && std::io::stdout().is_terminal()
}

pub fn print_status_default(groups: &[PackageStatus], color: bool) {
    for pkg in groups {
        let summary = format!("({}, {})", files_label(pkg.total), status_summary(pkg));

        if color {
            if pkg.modified == 0 && pkg.missing == 0 {
                println!("{} {}", pkg.name, summary.green());
            } else if pkg.missing > 0 {
                println!("{} {}", pkg.name, summary.red());
            } else {
                println!("{} {}", pkg.name, summary.yellow());
            }
        } else {
            println!("{} {}", pkg.name, summary);
        }

        for file in &pkg.files {
            match file.status {
                FileStatus::Modified => {
                    if color {
                        println!("  {} {}", "M".yellow(), file.display_path);
                    } else {
                        println!("  M {}", file.display_path);
                    }
                }
                FileStatus::Missing => {
                    if color {
                        println!("  {} {}", "!".red(), file.display_path);
                    } else {
                        println!("  ! {}", file.display_path);
                    }
                }
                FileStatus::Ok => {}
            }
        }
    }
}

pub fn print_status_verbose(groups: &[PackageStatus], color: bool) {
    for pkg in groups {
        let summary = format!("({}, {})", files_label(pkg.total), status_summary(pkg));

        if color {
            if pkg.modified == 0 && pkg.missing == 0 {
                println!("{} {}", pkg.name, summary.green());
            } else if pkg.missing > 0 {
                println!("{} {}", pkg.name, summary.red());
            } else {
                println!("{} {}", pkg.name, summary.yellow());
            }
        } else {
            println!("{} {}", pkg.name, summary);
        }

        for file in &pkg.files {
            match file.status {
                FileStatus::Ok => {
                    if color {
                        println!("  {} {}", "~".green(), file.display_path);
                    } else {
                        println!("  ~ {}", file.display_path);
                    }
                }
                FileStatus::Modified => {
                    if color {
                        println!("  {} {}", "M".yellow(), file.display_path);
                    } else {
                        println!("  M {}", file.display_path);
                    }
                }
                FileStatus::Missing => {
                    if color {
                        println!("  {} {}", "!".red(), file.display_path);
                    } else {
                        println!("  ! {}", file.display_path);
                    }
                }
            }
        }
    }
}

pub fn print_short(total: usize, modified: usize, missing: usize, color: bool) {
    let text = render_short(total, modified, missing);
    if text.is_empty() {
        return;
    }
    if color {
        if missing > 0 {
            print!("{}", text.red());
        } else {
            print!("{}", text.yellow());
        }
    } else {
        print!("{}", text);
    }
}

pub fn print_footer(total: usize, modified: usize, missing: usize, color: bool) {
    let text = render_footer(total, modified, missing);
    if color && modified == 0 && missing == 0 {
        print!("{}", text.green());
    } else {
        print!("{}", text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::EntryKind;
    use crate::state::DeployEntry;
    use std::path::PathBuf;

    fn make_entry(target: &str, package: &str, hash: &str) -> DeployEntry {
        DeployEntry {
            target: PathBuf::from(target),
            staged: PathBuf::from(format!("/staged{target}")),
            source: PathBuf::from(format!("/source{target}")),
            content_hash: hash.to_string(),
            kind: EntryKind::Base,
            package: package.to_string(),
        }
    }

    #[test]
    fn group_entries_by_package() {
        let entries = vec![
            make_entry("/home/user/.bashrc", "shell", "h1"),
            make_entry("/home/user/.zshrc", "shell", "h2"),
            make_entry("/home/user/.config/app.conf", "desktop", "h3"),
        ];
        let statuses = vec![FileStatus::Ok, FileStatus::Ok, FileStatus::Modified];
        let grouped = group_by_package(&entries, &statuses);

        assert_eq!(grouped.len(), 2);
        let desktop = grouped.iter().find(|g| g.name == "desktop").unwrap();
        assert_eq!(desktop.total, 1);
        assert_eq!(desktop.modified, 1);
        let shell = grouped.iter().find(|g| g.name == "shell").unwrap();
        assert_eq!(shell.total, 2);
        assert_eq!(shell.ok, 2);
    }

    #[test]
    fn packages_sorted_alphabetically() {
        let entries = vec![
            make_entry("/a", "zsh", "h1"),
            make_entry("/b", "bin", "h2"),
            make_entry("/c", "gaming", "h3"),
        ];
        let statuses = vec![FileStatus::Ok, FileStatus::Ok, FileStatus::Ok];
        let grouped = group_by_package(&entries, &statuses);
        let names: Vec<&str> = grouped.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["bin", "gaming", "zsh"]);
    }

    #[test]
    fn render_default_shows_package_headers() {
        let entries = vec![
            make_entry("/home/user/.bashrc", "shell", "h1"),
            make_entry("/home/user/.config/app.conf", "desktop", "h2"),
        ];
        let statuses = vec![FileStatus::Ok, FileStatus::Modified];
        let grouped = group_by_package(&entries, &statuses);
        let output = render_default(&grouped);
        assert!(output.contains("shell"));
        assert!(output.contains("desktop"));
        assert!(output.contains("1 modified"));
        assert!(output.contains("M "));
        assert!(output.contains("app.conf"));
    }

    #[test]
    fn render_default_hides_ok_files() {
        let entries = vec![make_entry("/home/user/.bashrc", "shell", "h1")];
        let statuses = vec![FileStatus::Ok];
        let grouped = group_by_package(&entries, &statuses);
        let output = render_default(&grouped);
        assert!(output.contains("shell"));
        assert!(output.contains("ok"));
        assert!(!output.contains(".bashrc"));
    }

    #[test]
    fn render_verbose_shows_all_files() {
        let entries = vec![
            make_entry("/home/user/.bashrc", "shell", "h1"),
            make_entry("/home/user/.zshrc", "shell", "h2"),
        ];
        let statuses = vec![FileStatus::Ok, FileStatus::Ok];
        let grouped = group_by_package(&entries, &statuses);
        let output = render_verbose(&grouped);
        assert!(output.contains(".bashrc"));
        assert!(output.contains(".zshrc"));
    }

    #[test]
    fn render_short_empty_when_clean() {
        let output = render_short(5, 0, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn render_short_shows_problems() {
        let output = render_short(10, 2, 1);
        assert!(output.contains("dotm:"));
        assert!(output.contains("2 modified"));
        assert!(output.contains("1 missing"));
    }

    #[test]
    fn render_footer_all_ok() {
        let output = render_footer(10, 0, 0);
        assert!(output.contains("10 managed"));
        assert!(output.contains("all ok"));
    }

    #[test]
    fn render_footer_with_problems() {
        let output = render_footer(10, 2, 1);
        assert!(output.contains("10 managed"));
        assert!(output.contains("2 modified"));
        assert!(output.contains("1 missing"));
    }
}
