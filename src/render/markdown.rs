// use colored::{Color, Colorize};
use crossterm::style::{Color, Stylize};
use syntect::highlighting::{Color as SyntectColor, FontStyle, Style, Theme};
use syntect::parsing::SyntaxSet;
use syntect::{easy::HighlightLines, parsing::SyntaxReference};

/// Monokai Extended
const MD_THEME: &[u8] = include_bytes!("../../assets/monokai-extended.theme.bin");
/// Comes from https://github.com/sharkdp/bat/raw/5e77ca37e89c873e4490b42ff556370dc5c6ba4f/assets/syntaxes.bin
const SYNTAXES: &[u8] = include_bytes!("../../assets/syntaxes.bin");

pub struct MarkdownRender {
    syntax_set: SyntaxSet,
    md_theme: Theme,
    code_color: Color,
    md_syntax: SyntaxReference,
    code_syntax: Option<SyntaxReference>,
    line_type: LineType,
}

impl MarkdownRender {
    pub fn new() -> Self {
        let syntax_set: SyntaxSet =
            bincode::deserialize_from(SYNTAXES).expect("invalid syntaxes binary");
        let md_theme: Theme = bincode::deserialize_from(MD_THEME).expect("invalid md_theme binary");
        let code_color = get_code_color(&md_theme);
        let md_syntax = syntax_set.find_syntax_by_extension("md").unwrap().clone();
        let line_type = LineType::Normal;
        Self {
            syntax_set,
            md_theme,
            code_color,
            md_syntax,
            code_syntax: None,
            line_type,
        }
    }

    pub fn render(&mut self, src: &str) -> String {
        src.split('\n')
            .map(|line| self.render_line(line).unwrap_or_else(|| line.to_string()))
            .collect::<Vec<String>>()
            .join("\n")
    }

    pub fn render_line(&mut self, line: &str) -> Option<String> {
        if let Some(lang) = detect_code_block(line) {
            match self.line_type {
                LineType::Normal | LineType::CodeEnd => {
                    self.line_type = LineType::CodeBegin;
                    self.code_syntax = if lang.is_empty() {
                        None
                    } else {
                        self.find_syntax(&lang).cloned()
                    };
                }
                LineType::CodeBegin | LineType::CodeInner => {
                    self.line_type = LineType::CodeEnd;
                    self.code_syntax = None;
                }
            }
            self.render_line_inner(line, &self.md_syntax)
        } else {
            match self.line_type {
                LineType::Normal => self.render_line_inner(line, &self.md_syntax),
                LineType::CodeEnd => {
                    self.line_type = LineType::Normal;
                    self.render_line_inner(line, &self.md_syntax)
                }
                LineType::CodeBegin => {
                    self.line_type = LineType::CodeInner;
                    self.render_code_line(line)
                }
                LineType::CodeInner => self.render_code_line(line),
            }
        }
    }

    pub fn render_line_stateless(&self, line: &str) -> String {
        let output = if detect_code_block(line).is_some() {
            self.render_line_inner(line, &self.md_syntax)
        } else {
            match self.line_type {
                LineType::Normal | LineType::CodeEnd => {
                    self.render_line_inner(line, &self.md_syntax)
                }
                _ => self.render_code_line(line),
            }
        };

        output.unwrap_or_else(|| line.to_string())
    }

    fn render_line_inner(&self, line: &str, syntax: &SyntaxReference) -> Option<String> {
        let mut highlighter = HighlightLines::new(syntax, &self.md_theme);
        let ranges = highlighter.highlight_line(line, &self.syntax_set).ok()?;
        Some(as_terminal_escaped(&ranges))
    }

    fn render_code_line(&self, line: &str) -> Option<String> {
        self.code_syntax
            .as_ref()
            .map(|syntax| self.render_line_inner(line, syntax))
            .unwrap_or_else(|| Some(format!("{}", line.with(self.code_color))))
    }

    fn find_syntax(&self, lang: &str) -> Option<&SyntaxReference> {
        self.syntax_set
            .find_syntax_by_token(lang)
            .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineType {
    Normal,
    CodeBegin,
    CodeInner,
    CodeEnd,
}

fn as_terminal_escaped(ranges: &[(Style, &str)]) -> String {
    let mut output = String::new();
    for (style, text) in ranges {
        let fg = blend_fg_color(style.foreground, style.background);
        let mut text = text.with(convert_color(fg));
        if style.font_style.contains(FontStyle::BOLD) {
            text = text.bold()
        }
        if style.font_style.contains(FontStyle::UNDERLINE) {
            text = text.underlined()
        }
        output.push_str(&text.to_string());
    }
    output
}

fn convert_color(c: SyntectColor) -> Color {
    Color::Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    }
}

fn blend_fg_color(fg: SyntectColor, bg: SyntectColor) -> SyntectColor {
    if fg.a == 0xff {
        return fg;
    }
    let ratio = fg.a as u32;
    let r = (fg.r as u32 * ratio + bg.r as u32 * (255 - ratio)) / 255;
    let g = (fg.g as u32 * ratio + bg.g as u32 * (255 - ratio)) / 255;
    let b = (fg.b as u32 * ratio + bg.b as u32 * (255 - ratio)) / 255;
    SyntectColor {
        r: r as u8,
        g: g as u8,
        b: b as u8,
        a: 255,
    }
}

fn detect_code_block(line: &str) -> Option<String> {
    if !line.starts_with("```") {
        return None;
    }
    let lang = line
        .chars()
        .skip(3)
        .take_while(|v| v.is_alphanumeric())
        .collect();
    Some(lang)
}

fn get_code_color(theme: &Theme) -> Color {
    let scope = theme.scopes.iter().find(|v| {
        v.scope
            .selectors
            .iter()
            .any(|v| v.path.scopes.iter().any(|v| v.to_string() == "string"))
    });
    scope
        .and_then(|v| v.style.foreground)
        .map(convert_color)
        .unwrap_or_else(|| Color::Yellow)
}

#[test]
fn test_assets() {
    let syntax_set: SyntaxSet = bincode::deserialize_from(SYNTAXES).expect("invalid syntaxes.bin");
    assert!(syntax_set.find_syntax_by_extension("md").is_some());
    let md_theme: Theme = bincode::deserialize_from(MD_THEME).expect("invalid md_theme binary");
    assert_eq!(md_theme.name, Some("Monokai Extended".into()));
}
