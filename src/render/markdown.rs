use crossterm::style::{Color, Stylize};
use lazy_static::lazy_static;
use std::collections::HashMap;
use syntect::highlighting::{Color as SyntectColor, FontStyle, Style, Theme};
use syntect::parsing::SyntaxSet;
use syntect::{easy::HighlightLines, parsing::SyntaxReference};

/// Monokai Extended
const MD_THEME: &[u8] = include_bytes!("../../assets/monokai-extended.theme.bin");
/// Comes from https://github.com/sharkdp/bat/raw/5e77ca37e89c873e4490b42ff556370dc5c6ba4f/assets/syntaxes.bin
const SYNTAXES: &[u8] = include_bytes!("../../assets/syntaxes.bin");

lazy_static! {
    static ref LANGE_MAPS: HashMap<String, String> = {
        let mut m = HashMap::new();
        m.insert("csharp".into(), "C#".into());
        m.insert("php".into(), "PHP Source".into());
        m
    };
}

pub struct MarkdownRender {
    syntax_set: SyntaxSet,
    md_theme: Theme,
    code_color: Color,
    md_syntax: SyntaxReference,
    code_syntax: Option<SyntaxReference>,
    prev_line_type: LineType,
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
            prev_line_type: line_type,
        }
    }

    pub fn render(&mut self, src: &str) -> String {
        src.split('\n')
            .map(|line| self.render_line(line).unwrap_or_else(|| line.to_string()))
            .collect::<Vec<String>>()
            .join("\n")
    }

    pub fn render_line_stateless(&self, line: &str) -> String {
        let output = if self.is_code_block() && detect_code_block(line).is_none() {
            self.render_code_line(line)
        } else {
            self.render_line_inner(line, &self.md_syntax)
        };
        output.unwrap_or_else(|| line.to_string())
    }

    pub fn is_code_block(&self) -> bool {
        matches!(
            self.prev_line_type,
            LineType::CodeBegin | LineType::CodeInner
        )
    }

    fn render_line(&mut self, line: &str) -> Option<String> {
        if let Some(lang) = detect_code_block(line) {
            match self.prev_line_type {
                LineType::Normal | LineType::CodeEnd => {
                    self.prev_line_type = LineType::CodeBegin;
                    self.code_syntax = if lang.is_empty() {
                        None
                    } else {
                        self.find_syntax(&lang).cloned()
                    };
                }
                LineType::CodeBegin | LineType::CodeInner => {
                    self.prev_line_type = LineType::CodeEnd;
                    self.code_syntax = None;
                }
            }
            self.render_line_inner(line, &self.md_syntax)
        } else {
            match self.prev_line_type {
                LineType::Normal => self.render_line_inner(line, &self.md_syntax),
                LineType::CodeEnd => {
                    self.prev_line_type = LineType::Normal;
                    self.render_line_inner(line, &self.md_syntax)
                }
                LineType::CodeBegin => {
                    if self.code_syntax.is_none() {
                        if let Some(syntax) = self.syntax_set.find_syntax_by_first_line(line) {
                            self.code_syntax = Some(syntax.clone());
                        }
                    }
                    self.prev_line_type = LineType::CodeInner;
                    self.render_code_line(line)
                }
                LineType::CodeInner => self.render_code_line(line),
            }
        }
    }

    fn render_line_inner(&self, line: &str, syntax: &SyntaxReference) -> Option<String> {
        let ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        let trimed_line = &line[ws.len()..];
        let mut highlighter = HighlightLines::new(syntax, &self.md_theme);
        let ranges = highlighter
            .highlight_line(trimed_line, &self.syntax_set)
            .ok()?;
        Some(format!("{ws}{}", as_terminal_escaped(&ranges)))
    }

    fn render_code_line(&self, line: &str) -> Option<String> {
        self.code_syntax
            .as_ref()
            .map(|syntax| self.render_line_inner(line, syntax))
            .unwrap_or_else(|| Some(format!("{}", line.with(self.code_color))))
    }

    fn find_syntax(&self, lang: &str) -> Option<&SyntaxReference> {
        if let Some(new_lang) = LANGE_MAPS.get(&lang.to_ascii_lowercase()) {
            self.syntax_set.find_syntax_by_name(new_lang)
        } else {
            self.syntax_set
                .find_syntax_by_token(lang)
                .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineType {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assets() {
        let syntax_set: SyntaxSet =
            bincode::deserialize_from(SYNTAXES).expect("invalid syntaxes.bin");
        assert!(syntax_set.find_syntax_by_extension("md").is_some());
        let md_theme: Theme = bincode::deserialize_from(MD_THEME).expect("invalid md_theme binary");
        assert_eq!(md_theme.name, Some("Monokai Extended".into()));
    }

    #[test]
    fn test_render() {
        let render = MarkdownRender::new();
        assert!(render.find_syntax("csharp").is_some());
    }
}
