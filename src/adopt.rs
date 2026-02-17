use anyhow::Result;
use crossterm::style::Stylize;
use similar::{ChangeTag, TextDiff};
use std::io::Write;

/// A single diff hunk representing a localized change between the original and modified file.
pub struct Hunk {
    /// The unified diff header (e.g., "@@ -1,3 +1,4 @@")
    pub header: String,
    /// Formatted hunk text for display (with +/- lines and context)
    pub display: String,
    /// Range of lines in the original text that this hunk covers (start index, exclusive end)
    pub old_range: (usize, usize),
    /// The replacement lines from the modified version
    pub new_lines: Vec<String>,
    /// The original lines being replaced
    pub old_lines: Vec<String>,
}

/// Compute the diff between `original` and `modified`, returning structured hunks.
pub fn extract_hunks(original: &str, modified: &str) -> Vec<Hunk> {
    let diff = TextDiff::from_lines(original, modified);
    let mut hunks = Vec::new();

    for group in diff.grouped_ops(3) {
        if group.is_empty() {
            continue;
        }

        // Compute the overall old/new ranges for this hunk group
        let first = &group[0];
        let last = &group[group.len() - 1];
        let old_start = first.old_range().start;
        let old_end = last.old_range().end;

        // Build the header
        let new_start = first.new_range().start;
        let new_end = last.new_range().end;
        let old_len = old_end - old_start;
        let new_len = new_end - new_start;
        let header = format!(
            "@@ -{},{} +{},{} @@",
            old_start + 1,
            old_len,
            new_start + 1,
            new_len
        );

        // Build display text and collect the full new-side lines for this hunk.
        // new_lines gets Equal + Insert lines (the full replacement when accepted).
        // old_lines gets Equal + Delete lines (should match original[old_start..old_end]).
        let mut display = String::new();
        display.push_str(&header);
        display.push('\n');

        let mut old_lines = Vec::new();
        let mut new_lines = Vec::new();

        for op in &group {
            for change in diff.iter_changes(op) {
                let line = change.to_string_lossy();
                let line_str = line.as_ref();
                match change.tag() {
                    ChangeTag::Equal => {
                        display.push_str(&format!(" {}", line_str));
                        if !line_str.ends_with('\n') {
                            display.push('\n');
                        }
                        old_lines.push(line_str.to_string());
                        new_lines.push(line_str.to_string());
                    }
                    ChangeTag::Delete => {
                        display.push_str(&format!("-{}", line_str));
                        if !line_str.ends_with('\n') {
                            display.push('\n');
                        }
                        old_lines.push(line_str.to_string());
                    }
                    ChangeTag::Insert => {
                        display.push_str(&format!("+{}", line_str));
                        if !line_str.ends_with('\n') {
                            display.push('\n');
                        }
                        new_lines.push(line_str.to_string());
                    }
                }
            }
        }

        hunks.push(Hunk {
            header,
            display,
            old_range: (old_start, old_end),
            new_lines,
            old_lines,
        });
    }

    hunks
}

/// Apply selected hunks to the original text, producing the patched result.
///
/// For each hunk, if `accepted[i]` is true, the old lines in that region are replaced
/// with the new lines from the modified version. If false, the original lines are kept.
/// Lines outside any hunk are always preserved from the original.
pub fn apply_hunks(original: &str, hunks: &[Hunk], accepted: &[bool]) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let mut result = Vec::new();
    let mut pos = 0;

    for (i, hunk) in hunks.iter().enumerate() {
        let (hunk_start, hunk_end) = hunk.old_range;

        // Copy lines before this hunk (between previous hunk end and this hunk start)
        for line in &orig_lines[pos..hunk_start] {
            result.push((*line).to_string());
        }

        if accepted[i] {
            // Use the new lines from the modified version
            for line in &hunk.new_lines {
                // Strip trailing newline if present since we rejoin with \n
                result.push(line.strip_suffix('\n').unwrap_or(line).to_string());
            }
        } else {
            // Keep the original lines
            for line in &orig_lines[hunk_start..hunk_end] {
                result.push((*line).to_string());
            }
        }

        pos = hunk_end;
    }

    // Copy any remaining lines after the last hunk
    for line in &orig_lines[pos..] {
        result.push((*line).to_string());
    }

    let mut output = result.join("\n");
    // Preserve trailing newline if original had one
    if original.ends_with('\n') {
        output.push('\n');
    }
    output
}

/// Interactively prompt the user to accept or reject each hunk of changes.
///
/// Returns `Some(patched_content)` if any hunks were accepted, `None` if all were
/// rejected or the user quit early.
pub fn interactive_adopt(file_label: &str, original: &str, modified: &str) -> Result<Option<String>> {
    let hunks = extract_hunks(original, modified);
    if hunks.is_empty() {
        return Ok(None);
    }

    let mut accepted = vec![false; hunks.len()];
    let mut any_accepted = false;

    println!("\n--- {}", file_label);

    for (i, hunk) in hunks.iter().enumerate() {
        println!();
        println!("Hunk {}/{}", i + 1, hunks.len());

        // Display the hunk with colored output
        for line in hunk.display.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                println!("{}", line.green());
            } else if line.starts_with('-') && !line.starts_with("---") {
                println!("{}", line.red());
            } else if line.starts_with("@@") {
                println!("{}", line.cyan());
            } else {
                println!("{}", line);
            }
        }

        // Prompt for action
        loop {
            print!("Accept this change? [y/n/a/q] ");
            std::io::stdout().flush()?;

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let choice = input.trim().to_lowercase();

            match choice.as_str() {
                "y" | "yes" => {
                    accepted[i] = true;
                    any_accepted = true;
                    break;
                }
                "n" | "no" => {
                    break;
                }
                "a" | "all" => {
                    for item in accepted.iter_mut().take(hunks.len()).skip(i) {
                        *item = true;
                    }
                    let result = apply_hunks(original, &hunks, &accepted);
                    return Ok(Some(result));
                }
                "q" | "quit" => {
                    if any_accepted {
                        let result = apply_hunks(original, &hunks, &accepted);
                        return Ok(Some(result));
                    }
                    return Ok(None);
                }
                _ => {
                    println!("  y = accept, n = reject, a = accept all remaining, q = quit");
                }
            }
        }
    }

    if any_accepted {
        let result = apply_hunks(original, &hunks, &accepted);
        Ok(Some(result))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_hunks_finds_changes() {
        let original = "line1\nline2\nline3\nline4\nline5\n";
        let modified = "line1\nchanged2\nline3\nline4\nnew5\n";
        let hunks = extract_hunks(original, modified);
        assert!(!hunks.is_empty());
    }

    #[test]
    fn extract_hunks_empty_for_identical() {
        let content = "line1\nline2\nline3\n";
        let hunks = extract_hunks(content, content);
        assert!(hunks.is_empty());
    }

    #[test]
    fn apply_all_hunks_produces_modified() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nchanged2\nline3\n";
        let hunks = extract_hunks(original, modified);
        let accepted: Vec<bool> = hunks.iter().map(|_| true).collect();
        let result = apply_hunks(original, &hunks, &accepted);
        assert_eq!(result, modified);
    }

    #[test]
    fn reject_all_hunks_produces_original() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nchanged2\nline3\n";
        let hunks = extract_hunks(original, modified);
        let accepted: Vec<bool> = hunks.iter().map(|_| false).collect();
        let result = apply_hunks(original, &hunks, &accepted);
        assert_eq!(result, original);
    }

    #[test]
    fn apply_selective_hunks() {
        // With enough separation between changes, they should be separate hunks
        let original = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np\n";
        let modified = "a\nB\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\nO\np\n";
        let hunks = extract_hunks(original, modified);

        if hunks.len() >= 2 {
            // Accept only the first hunk
            let mut accepted = vec![false; hunks.len()];
            accepted[0] = true;
            let result = apply_hunks(original, &hunks, &accepted);
            // First change applied (b -> B), second not (o stays o)
            assert!(result.contains("\nB\n"));
            assert!(result.contains("\no\n"));
        }
    }

    #[test]
    fn apply_hunks_with_additions() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nline2\nnew_line\nline3\n";
        let hunks = extract_hunks(original, modified);
        let accepted: Vec<bool> = hunks.iter().map(|_| true).collect();
        let result = apply_hunks(original, &hunks, &accepted);
        assert_eq!(result, modified);
    }

    #[test]
    fn apply_hunks_with_deletions() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nline3\n";
        let hunks = extract_hunks(original, modified);
        let accepted: Vec<bool> = hunks.iter().map(|_| true).collect();
        let result = apply_hunks(original, &hunks, &accepted);
        assert_eq!(result, modified);
    }

    #[test]
    fn reject_hunks_with_deletions_preserves_original() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nline3\n";
        let hunks = extract_hunks(original, modified);
        let accepted: Vec<bool> = hunks.iter().map(|_| false).collect();
        let result = apply_hunks(original, &hunks, &accepted);
        assert_eq!(result, original);
    }

    #[test]
    fn hunk_header_present() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nchanged2\nline3\n";
        let hunks = extract_hunks(original, modified);
        assert!(!hunks.is_empty());
        assert!(hunks[0].header.starts_with("@@"));
    }

    #[test]
    fn hunk_display_contains_changes() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nchanged2\nline3\n";
        let hunks = extract_hunks(original, modified);
        assert!(!hunks.is_empty());
        assert!(hunks[0].display.contains("-line2"));
        assert!(hunks[0].display.contains("+changed2"));
    }
}
