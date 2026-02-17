use similar::TextDiff;

pub fn format_unified_diff(original: &str, modified: &str, label_a: &str, label_b: &str) -> String {
    let diff = TextDiff::from_lines(original, modified);
    let mut output = String::new();

    output.push_str(&format!("--- {}\n", label_a));
    output.push_str(&format!("+++ {}\n", label_b));

    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        output.push_str(&format!("{}", hunk));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_diff_shows_unified_output() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nmodified_line2\nline3\nnew_line4\n";
        let output = format_unified_diff(
            original,
            modified,
            "deployed: .config/app.conf",
            "current: .config/app.conf",
        );
        assert!(output.contains("--- deployed:"));
        assert!(output.contains("+++ current:"));
        assert!(output.contains("-line2"));
        assert!(output.contains("+modified_line2"));
        assert!(output.contains("+new_line4"));
    }

    #[test]
    fn no_diff_produces_empty_output() {
        let content = "same\ncontent\n";
        let output = format_unified_diff(content, content, "a", "b");
        // Headers are still present but no hunks
        assert!(output.starts_with("--- a\n+++ b\n"));
        // No @@ hunk headers
        assert!(!output.contains("@@"));
    }
}
