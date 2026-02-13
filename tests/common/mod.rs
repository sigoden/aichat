// Common test utilities for aichat rendering tests

use std::fs;

/// Read a test fixture file
pub fn read_fixture(name: &str) -> String {
    let path = format!("tests/fixtures/markdown/{}", name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", path, e))
}

/// Read a benchmark fixture file
pub fn read_benchmark_fixture(name: &str) -> String {
    let path = format!("tests/fixtures/benchmark/{}", name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read benchmark fixture {}: {}", path, e))
}

/// Strip ANSI escape codes from a string for comparison
pub fn strip_ansi(s: &str) -> String {
    // Simple ANSI stripping - matches ESC [ ... m sequences
    let re = fancy_regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

/// Compare two strings, ignoring ANSI codes
pub fn compare_output(actual: &str, expected: &str) -> bool {
    strip_ansi(actual) == strip_ansi(expected)
}

/// Count lines in a string
pub fn count_lines(s: &str) -> usize {
    s.lines().count()
}

/// Check if output contains a substring (ignoring ANSI codes)
pub fn contains_text(output: &str, text: &str) -> bool {
    strip_ansi(output).contains(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        let input = "\x1b[1mBold\x1b[0m text";
        let expected = "Bold text";
        assert_eq!(strip_ansi(input), expected);
    }

    #[test]
    fn test_count_lines() {
        let text = "Line 1\nLine 2\nLine 3";
        assert_eq!(count_lines(text), 3);
    }

    #[test]
    fn test_contains_text() {
        let output = "\x1b[1mHello\x1b[0m World";
        assert!(contains_text(output, "Hello World"));
    }
}
