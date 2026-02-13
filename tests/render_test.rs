// Integration tests for markdown rendering
// This file tests the current MarkdownRender implementation as a baseline

mod common;

use common::{count_lines, read_fixture};

#[test]
fn test_basic_elements_fixture_exists() {
    let content = read_fixture("basic-elements.md");
    assert!(!content.is_empty(), "basic-elements.md should not be empty");
    assert!(content.contains("# H1 Heading"), "Should contain H1 heading");
    assert!(content.contains("**bold text**"), "Should contain bold text");
}

#[test]
fn test_code_blocks_fixture_exists() {
    let content = read_fixture("code-blocks.md");
    assert!(!content.is_empty(), "code-blocks.md should not be empty");
    assert!(content.contains("```rust"), "Should contain Rust code block");
    assert!(content.contains("```python"), "Should contain Python code block");
}

#[test]
fn test_tables_fixture_exists() {
    let content = read_fixture("tables.md");
    assert!(!content.is_empty(), "tables.md should not be empty");
    assert!(content.contains("| Name | Age | City |"), "Should contain table header");
    assert!(content.contains("|------|-----|------|"), "Should contain table separator");
}

#[test]
fn test_lists_fixture_exists() {
    let content = read_fixture("lists.md");
    assert!(!content.is_empty(), "lists.md should not be empty");
    assert!(content.contains("- Item 1"), "Should contain unordered list");
    assert!(content.contains("1. First item"), "Should contain ordered list");
}

#[test]
fn test_complex_fixture_exists() {
    let content = read_fixture("complex.md");
    assert!(!content.is_empty(), "complex.md should not be empty");
    assert!(content.contains("| Language | Paradigm |"), "Should contain table");
    assert!(content.contains("```rust"), "Should contain code block");
}

#[test]
fn test_fixture_line_counts() {
    // Verify fixtures have reasonable line counts
    let basic = read_fixture("basic-elements.md");
    assert!(count_lines(&basic) > 10, "basic-elements should have > 10 lines");

    let code = read_fixture("code-blocks.md");
    assert!(count_lines(&code) > 50, "code-blocks should have > 50 lines");

    let tables = read_fixture("tables.md");
    assert!(count_lines(&tables) > 50, "tables should have > 50 lines");

    let lists = read_fixture("lists.md");
    assert!(count_lines(&lists) > 50, "lists should have > 50 lines");

    let complex = read_fixture("complex.md");
    assert!(count_lines(&complex) > 100, "complex should have > 100 lines");
}

// Note: Actual rendering tests are in src/render/streamdown_adapter.rs as unit tests
// since aichat is a binary crate without a lib target.
