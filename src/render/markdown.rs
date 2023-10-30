use anyhow::{anyhow, Context, Result};
use crossterm::style::{Color, Stylize};
use crossterm::terminal;
use lazy_static::lazy_static;
use std::collections::HashMap;
use syntect::highlighting::{Color as SyntectColor, FontStyle, Style, Theme};
use syntect::parsing::SyntaxSet;
use syntect::{easy::HighlightLines, parsing::SyntaxReference};

/// Monokai Extended
const MD_THEME: &[u8] = include_bytes!("../../assets/monokai-extended.theme.bin");
const MD_THEME_LIGHT: &[u8] = include_bytes!("../../assets/monokai-extended-light.theme.bin");
#[allow(clippy::doc_markdown)]
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

#[allow(clippy::module_name_repetitions)]
pub struct MarkdownRender {
    options: RenderOptions,
    syntax_set: SyntaxSet,
    md_theme: Option<Theme>,
    code_color: Option<Color>,
    md_syntax: SyntaxReference,
    code_syntax: Option<SyntaxReference>,
    prev_line_type: LineType,
    wrap_width: Option<u16>,
}

impl MarkdownRender {
    pub fn init(options: RenderOptions) -> Result<Self> {
        let syntax_set: SyntaxSet = bincode::deserialize_from(SYNTAXES)
            .with_context(|| "MarkdownRender: invalid syntaxes binary")?;

        let md_theme: Option<Theme> = match (options.highlight, options.light_theme) {
            (false, _) => None,
            (true, false) => Some(
                bincode::deserialize_from(MD_THEME)
                    .with_context(|| "MarkdownRender: invalid theme binary")?,
            ),
            (true, true) => Some(
                bincode::deserialize_from(MD_THEME_LIGHT)
                    .expect("MarkdownRender: invalid theme binary"),
            ),
        };
        let code_color = md_theme.as_ref().map(get_code_color);
        let md_syntax = syntax_set.find_syntax_by_extension("md").unwrap().clone();
        let line_type = LineType::Normal;
        let wrap_width = match options.wrap.as_deref() {
            None => None,
            Some("auto") => {
                let (columns, _) =
                    terminal::size().with_context(|| "Unable to get terminal size")?;
                Some(columns)
            }
            Some(value) => {
                let (columns, _) =
                    terminal::size().with_context(|| "Unable to get terminal size")?;
                let value = value
                    .parse::<u16>()
                    .map_err(|_| anyhow!("Invalid wrap value"))?;
                Some(columns.min(value))
            }
        };
        Ok(Self {
            syntax_set,
            md_theme,
            code_color,
            md_syntax,
            code_syntax: None,
            prev_line_type: line_type,
            wrap_width,
            options,
        })
    }

    pub fn render(&mut self, text: &str) -> String {
        text.split('\n')
            .map(|line| self.render_line(line).unwrap_or_else(|| line.to_string()))
            .collect::<Vec<String>>()
            .join("\n")
    }

    pub const fn is_code_block(&self) -> bool {
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
            self.highligh_line(line, &self.md_syntax, false)
        } else {
            match self.prev_line_type {
                LineType::Normal => self.highligh_line(line, &self.md_syntax, false),
                LineType::CodeEnd => {
                    self.prev_line_type = LineType::Normal;
                    self.highligh_line(line, &self.md_syntax, false)
                }
                LineType::CodeBegin => {
                    if self.code_syntax.is_none() {
                        if let Some(syntax) = self.syntax_set.find_syntax_by_first_line(line) {
                            self.code_syntax = Some(syntax.clone());
                        }
                    }
                    self.prev_line_type = LineType::CodeInner;
                    self.highlint_code_line(line)
                }
                LineType::CodeInner => self.highlint_code_line(line),
            }
        }
    }

    fn highligh_line(&self, line: &str, syntax: &SyntaxReference, is_code: bool) -> Option<String> {
        let ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        let trimed_line: &str = &line[ws.len()..];
        let line = match &self.md_theme {
            Some(theme) => {
                let mut highlighter = HighlightLines::new(syntax, theme);
                let ranges = highlighter
                    .highlight_line(trimed_line, &self.syntax_set)
                    .ok()?;
                Some(format!("{ws}{}", as_terminal_escaped(&ranges)))
            }
            None => Some(trimed_line.to_string()),
        };
        let line = line?;
        Some(self.wrap_line(line, is_code))
    }

    fn highlint_code_line(&self, line: &str) -> Option<String> {
        match self.code_color {
            None => Some(self.wrap_line(line.to_string(), true)),
            Some(color) => self.code_syntax.as_ref().map_or_else(
                || Some(format!("{}", line.with(color))),
                |syntax| self.highligh_line(line, syntax, true),
            ),
        }
    }

    fn wrap_line(&self, line: String, is_code: bool) -> String {
        if let Some(width) = self.wrap_width {
            if is_code && !self.options.wrap_code {
                return line;
            }
            textwrap::wrap(&line, width as usize).join("\n")
        } else {
            line
        }
    }

    fn find_syntax(&self, lang: &str) -> Option<&SyntaxReference> {
        #[allow(clippy::option_if_let_else)]
        if let Some(new_lang) = LANGE_MAPS.get(&lang.to_ascii_lowercase()) {
            self.syntax_set.find_syntax_by_name(new_lang)
        } else {
            self.syntax_set
                .find_syntax_by_token(lang)
                .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub highlight: bool,
    pub light_theme: bool,
    pub wrap: Option<String>,
    pub wrap_code: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            highlight: true,
            light_theme: false,
            wrap: None,
            wrap_code: false,
        }
    }
}

impl RenderOptions {
    pub(crate) fn new(
        highlight: bool,
        light_theme: bool,
        wrap: Option<String>,
        wrap_code: bool,
    ) -> Self {
        Self {
            highlight,
            light_theme,
            wrap,
            wrap_code,
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
            text = text.bold();
        }
        if style.font_style.contains(FontStyle::UNDERLINE) {
            text = text.underlined();
        }
        output.push_str(&text.to_string());
    }
    output
}

const fn convert_color(c: SyntectColor) -> Color {
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
    let ratio = u32::from(fg.a);
    let r = (u32::from(fg.r) * ratio + u32::from(bg.r) * (255 - ratio)) / 255;
    let g = (u32::from(fg.g) * ratio + u32::from(bg.g) * (255 - ratio)) / 255;
    let b = (u32::from(fg.b) * ratio + u32::from(bg.b) * (255 - ratio)) / 255;
    SyntectColor {
        r: u8::try_from(r).unwrap_or(u8::MAX),
        g: u8::try_from(g).unwrap_or(u8::MAX),
        b: u8::try_from(b).unwrap_or(u8::MAX),
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
        .map_or_else(|| Color::Yellow, convert_color)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = r#"
To unzip a file in Rust, you can use the `zip` crate. Here's an example code that shows how to unzip a file:

```rust
use std::fs::File;

fn unzip_file(path: &str, output_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    todo!()
}
```
"#;

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
        let options = RenderOptions::default();
        let render = MarkdownRender::init(options).unwrap();
        assert!(render.find_syntax("csharp").is_some());
    }

    #[test]
    fn no_theme() {
        let options = RenderOptions {
            highlight: false,
            ..Default::default()
        };
        let mut render = MarkdownRender::init(options).unwrap();
        let output = render.render(TEXT);
        assert_eq!(TEXT, output);
    }
}
