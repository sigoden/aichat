use crate::render::RenderOptions;

use anyhow::{Context, Result};
use streamdown_parser::Parser;
use streamdown_render::{RenderState, RenderStyle, Renderer};
use syntect::highlighting::Theme;

/// Streamdown-based markdown renderer adapter.
///
/// This adapter bridges aichat's rendering system with streamdown-rs,
/// providing full markdown rendering capabilities including tables,
/// lists, headings, and more.
pub struct StreamdownRenderer {
    parser: Parser,
    width: usize,
    style: RenderStyle,
    options: RenderOptions,
    render_state: RenderState,
}

impl StreamdownRenderer {
    /// Create a new StreamdownRenderer from RenderOptions.
    pub fn new(options: RenderOptions) -> Result<Self> {
        let parser = Parser::new();

        // Determine terminal width
        let width = determine_width(&options)?;

        // Convert RenderOptions to RenderStyle
        let style = convert_render_style(&options);

        Ok(Self {
            parser,
            width,
            style,
            options,
            render_state: RenderState::default(),
        })
    }

    /// Render complete markdown text (static rendering).
    ///
    /// This method processes the entire text at once, suitable for
    /// rendering complete markdown documents.
    pub fn render(&mut self, text: &str) -> Result<String> {
        let mut output = Vec::new();

        {
            let mut renderer = Renderer::with_style(&mut output, self.width, self.style.clone());

            // Pass the actual theme object for syntax highlighting
            if let Some(theme) = &self.options.theme {
                renderer.set_custom_theme(theme.clone());
            }

            // Strip token backgrounds so code_bg controls the background uniformly
            renderer.set_highlight_background(parse_hex_rgb(&self.style.code_bg));

            // Restore state from previous call
            renderer.restore_state(self.render_state.clone());

            // Parse and render line by line
            for line in text.lines() {
                for event in self.parser.parse_line(line) {
                    renderer.render_event(&event)
                        .with_context(|| "Failed to render event")?;
                }
            }

            // Save state for next call
            self.render_state = renderer.save_state();
        }

        String::from_utf8(output).with_context(|| "Invalid UTF-8 in rendered output")
    }

    /// Render a single line (streaming rendering).
    ///
    /// This method is designed for streaming scenarios where markdown
    /// arrives line by line. The parser maintains internal state across
    /// calls to handle multi-line structures correctly.
    pub fn render_line(&mut self, line: &str) -> Result<String> {
        let mut output = Vec::new();

        {
            let mut renderer = Renderer::with_style(&mut output, self.width, self.style.clone());

            // Pass the actual theme object for syntax highlighting
            if let Some(theme) = &self.options.theme {
                renderer.set_custom_theme(theme.clone());
            }

            // Strip token backgrounds so code_bg controls the background uniformly
            renderer.set_highlight_background(parse_hex_rgb(&self.style.code_bg));

            // Restore state from previous call
            renderer.restore_state(self.render_state.clone());

            // Parse and render the line
            for event in self.parser.parse_line(line) {
                renderer.render_event(&event)
                    .with_context(|| "Failed to render event")?;
            }

            // Save state for next call
            self.render_state = renderer.save_state();
        }

        String::from_utf8(output).with_context(|| "Invalid UTF-8 in rendered output")
    }
}

/// Determine terminal width from RenderOptions.
fn determine_width(options: &RenderOptions) -> Result<usize> {
    let width = match options.wrap.as_deref() {
        None => 80, // Default width
        Some("auto") => {
            // Auto-detect terminal width
            crossterm::terminal::size()
                .map(|(cols, _)| cols as usize)
                .unwrap_or(80)
        }
        Some(value) => {
            // Parse explicit width
            value.parse::<usize>()
                .with_context(|| format!("Invalid wrap value: {}", value))?
        }
    };

    Ok(width.max(40)) // Minimum width of 40
}

/// Convert aichat's RenderOptions to streamdown's RenderStyle.
///
/// This function maps color values from syntect's Theme to streamdown's
/// color scheme. If no theme is provided, default colors are used.
fn convert_render_style(options: &RenderOptions) -> RenderStyle {
    if let Some(theme) = &options.theme {
        // Extract colors from syntect theme
        extract_colors_from_theme(theme, options.truecolor)
    } else {
        // Use default streamdown colors
        RenderStyle::default()
    }
}

/// Extract colors from syntect Theme and create RenderStyle.
///
/// Maps syntect theme colors to streamdown's RenderStyle structure.
/// The RenderStyle includes colors for headings, code blocks, tables, etc.
fn extract_colors_from_theme(theme: &Theme, _truecolor: bool) -> RenderStyle {
    use syntect::highlighting::Color as SyntectColor;

    // Helper to convert syntect Color to hex string
    let color_to_hex = |color: SyntectColor| -> String {
        format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
    };

    // Extract foreground color for general text
    let fg_color = theme.settings.foreground
        .map(color_to_hex)
        .unwrap_or_else(|| "#ffffff".to_string());

    // Extract background color
    let bg_color = theme.settings.background
        .map(color_to_hex)
        .unwrap_or_else(|| "#000000".to_string());

    // Find specific scope colors
    let find_scope_color = |scope_name: &str| -> String {
        theme.scopes.iter()
            .find(|s| s.scope.selectors.iter().any(|sel| {
                sel.path.scopes.iter().any(|sc| sc.to_string().contains(scope_name))
            }))
            .and_then(|s| s.style.foreground)
            .map(color_to_hex)
            .unwrap_or_else(|| fg_color.clone())
    };

    // Map syntect theme colors to streamdown's RenderStyle
    RenderStyle {
        // Headings - use different colors for hierarchy
        h1: find_scope_color("markup.heading.1"),
        h2: find_scope_color("markup.heading.2"),
        h3: find_scope_color("markup.heading.3"),
        h4: find_scope_color("markup.heading.4"),
        h5: find_scope_color("markup.heading.5"),
        h6: find_scope_color("markup.heading.6"),

        // Code blocks - subtle background shift to distinguish from terminal bg
        // Keep close to original theme bg to preserve syntax highlight contrast
        code_bg: adjust_bg_contrast(&bg_color, 0.12),
        code_label: find_scope_color("entity.name.function"),

        // Lists
        bullet: find_scope_color("punctuation"),

        // Tables
        table_header_bg: adjust_bg_contrast(&bg_color, 0.1),
        table_border: find_scope_color("comment"),

        // Borders and decorations
        blockquote_border: find_scope_color("comment"),
        think_border: find_scope_color("comment"),
        hr: find_scope_color("comment"),

        // Links and references
        link_url: find_scope_color("markup.underline.link"),
        image_marker: find_scope_color("entity.name.tag"),
        footnote: find_scope_color("comment"),
    }
}

/// Adjust a background color for contrast based on theme brightness.
///
/// For dark themes: lightens towards white.
/// For light themes: darkens towards black.
fn adjust_bg_contrast(bg_hex: &str, factor: f32) -> String {
    let factor = factor.clamp(0.0, 1.0);
    let hex = bg_hex.trim_start_matches('#');
    if hex.len() != 6 {
        return bg_hex.to_string();
    }

    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);

    // Perceived luminance (ITU-R BT.601)
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    let is_dark = luma < 128.0;

    let adjust = |c: u8| -> u8 {
        let c = c as f32;
        if is_dark {
            (c + (255.0 - c) * factor).min(255.0) as u8
        } else {
            (c * (1.0 - factor)).max(0.0) as u8
        }
    };

    format!("#{:02x}{:02x}{:02x}", adjust(r), adjust(g), adjust(b))
}

/// Parse a hex color string to RGB tuple.
fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_width_default() {
        let options = RenderOptions::default();
        let width = determine_width(&options).unwrap();
        assert_eq!(width, 80);
    }

    #[test]
    fn test_determine_width_explicit() {
        let options = RenderOptions {
            wrap: Some("100".to_string()),
            ..Default::default()
        };
        let width = determine_width(&options).unwrap();
        assert_eq!(width, 100);
    }

    #[test]
    fn test_determine_width_minimum() {
        let options = RenderOptions {
            wrap: Some("20".to_string()),
            ..Default::default()
        };
        let width = determine_width(&options).unwrap();
        assert_eq!(width, 40); // Minimum width enforced
    }

    #[test]
    fn test_render_simple_text() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let text = "Hello, world!";
        let output = renderer.render(text).unwrap();

        // Output should contain the text (may have ANSI codes)
        assert!(output.contains("Hello"));
    }

    #[test]
    fn test_render_heading() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let text = "# Hello World";
        let output = renderer.render(text).unwrap();

        // Output should contain the heading text
        assert!(output.contains("Hello World"));
    }

    #[test]
    fn test_render_code_block() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let text = "```rust\nfn main() {}\n```";
        let output = renderer.render(text).unwrap();

        // Debug: print the actual output
        eprintln!("Output: {:?}", output);

        // Output should contain the code (may have ANSI codes, so check for substring)
        assert!(output.contains("main"), "Output should contain 'main', got: {}", output);
    }

    #[test]
    fn test_render_line_streaming() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        // Simulate streaming input
        let line1 = "# Heading";
        let line2 = "Some text";

        let output1 = renderer.render_line(line1).unwrap();
        let output2 = renderer.render_line(line2).unwrap();

        assert!(output1.contains("Heading"));
        assert!(output2.contains("Some text"));
    }

    // Additional tests for Phase 7: Testing and Optimization

    #[test]
    fn test_render_table() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let table = "| Name | Age |\n|------|-----|\n| Alice | 30 |\n| Bob | 25 |";
        let output = renderer.render(table).unwrap();

        // Verify table content is present
        assert!(output.contains("Name"));
        assert!(output.contains("Age"));
        assert!(output.contains("Alice"));
        assert!(output.contains("Bob"));
    }

    #[test]
    fn test_render_list() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let list = "- Item 1\n- Item 2\n  - Nested 1\n  - Nested 2";
        let output = renderer.render(list).unwrap();

        assert!(output.contains("Item 1"));
        assert!(output.contains("Item 2"));
        assert!(output.contains("Nested 1"));
    }

    #[test]
    fn test_render_inline_elements() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let inline = "**bold** *italic* [link](https://example.com)";
        let output = renderer.render(inline).unwrap();

        assert!(output.contains("bold"));
        assert!(output.contains("italic"));
        assert!(output.contains("link"));
    }

    #[test]
    fn test_render_empty_input() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let output = renderer.render("").unwrap();
        assert!(output.is_empty() || output.trim().is_empty());
    }

    #[test]
    fn test_render_long_line() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let long_line = "a".repeat(2000);
        let output = renderer.render(&long_line).unwrap();
        assert!(output.contains("aaa"));
    }

    #[test]
    fn test_render_unicode() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let unicode = "中文测试 🚀 Emoji";
        let output = renderer.render(unicode).unwrap();
        assert!(output.contains("中文测试"));
        assert!(output.contains("🚀"));
    }

    #[test]
    fn test_render_special_characters() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let special = "< > & \" '";
        let output = renderer.render(special).unwrap();
        // Should handle special characters without crashing
        assert!(!output.is_empty());
    }

    #[test]
    fn test_render_multiple_headings() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let headings = "# H1\n## H2\n### H3\n#### H4\n##### H5\n###### H6";
        let output = renderer.render(headings).unwrap();

        assert!(output.contains("H1"));
        assert!(output.contains("H2"));
        assert!(output.contains("H3"));
        assert!(output.contains("H4"));
        assert!(output.contains("H5"));
        assert!(output.contains("H6"));
    }

    #[test]
    fn test_render_blockquote() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let quote = "> This is a quote\n> Second line";
        let output = renderer.render(quote).unwrap();

        assert!(output.contains("This is a quote"));
        assert!(output.contains("Second line"));
    }

    #[test]
    fn test_render_horizontal_rule() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let hr = "---";
        let output = renderer.render(hr).unwrap();

        // Should produce some output (may be Unicode line)
        assert!(!output.is_empty());
    }

    #[test]
    fn test_render_mixed_content() {
        let options = RenderOptions::default();
        let mut renderer = StreamdownRenderer::new(options).unwrap();

        let mixed = "# Title\n\nSome **bold** text.\n\n- List item\n\n```rust\nfn main() {}\n```";
        let output = renderer.render(mixed).unwrap();

        assert!(output.contains("Title"));
        assert!(output.contains("bold"));
        assert!(output.contains("List item"));
        assert!(output.contains("main"));
    }

    #[test]
    fn test_adjust_bg_contrast() {
        // Dark background → lightens
        assert_eq!(adjust_bg_contrast("#000000", 0.3), "#4c4c4c");
        // Light background → darkens
        assert_eq!(adjust_bg_contrast("#ffffff", 0.3), "#b2b2b2");
        // Monokai dark bg → lightens
        assert_eq!(adjust_bg_contrast("#272822", 0.3), "#676864");
        // Mid-light → darkens
        assert_eq!(adjust_bg_contrast("#fafafa", 0.3), "#afafaf");
    }
}
