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
/// Comes from https://github.com/sharkdp/bat/raw/5e77ca37e89c873e4490b42ff556370dc5c6ba4f/assets/syntaxes.bin
const SYNTAXES: &[u8] = include_bytes!("../../assets/syntaxes.bin");

lazy_static! {
    static ref LANG_MAPS: HashMap<String, String> = {
        let mut m = HashMap::new();
        m.insert("csharp".into(), "C#".into());
        m.insert("php".into(), "PHP Source".into());
        m
    };
}

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
            Some(value) => match terminal::size() {
                Ok((columns, _)) => {
                    if value == "auto" {
                        Some(columns)
                    } else {
                        let value = value
                            .parse::<u16>()
                            .map_err(|_| anyhow!("Invalid wrap value"))?;
                        Some(columns.min(value))
                    }
                }
                Err(_) => None,
            },
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

    pub(crate) const fn is_code(&self) -> bool {
        matches!(
            self.prev_line_type,
            LineType::CodeBegin | LineType::CodeInner
        )
    }

    pub fn render(&mut self, text: &str) -> String {
        text.split('\n')
            .map(|line| self.render_line_mut(line))
            .collect::<Vec<String>>()
            .join("\n")
    }

    pub fn render_line(&self, line: &str) -> String {
        let (_, code_syntax, is_code) = self.check_line(line);
        if is_code {
            self.highlight_code_line(line, &code_syntax)
        } else {
            self.highlight_line(line, &self.md_syntax, false)
        }
    }

    fn render_line_mut(&mut self, line: &str) -> String {
        let (line_type, code_syntax, is_code) = self.check_line(line);
        let output = if is_code {
            self.highlight_code_line(line, &code_syntax)
        } else {
            self.highlight_line(line, &self.md_syntax, false)
        };
        self.prev_line_type = line_type;
        self.code_syntax = code_syntax;
        output
    }

    fn check_line(&self, line: &str) -> (LineType, Option<SyntaxReference>, bool) {
        let mut line_type = self.prev_line_type;
        let mut code_syntax = self.code_syntax.clone();
        let mut is_code = false;
        if let Some(lang) = detect_code_block(line) {
            match line_type {
                LineType::Normal | LineType::CodeEnd => {
                    line_type = LineType::CodeBegin;
                    code_syntax = if lang.is_empty() {
                        None
                    } else {
                        self.find_syntax(&lang).cloned()
                    };
                }
                LineType::CodeBegin | LineType::CodeInner => {
                    line_type = LineType::CodeEnd;
                    code_syntax = None;
                }
            }
        } else {
            match line_type {
                LineType::Normal => {}
                LineType::CodeEnd => {
                    line_type = LineType::Normal;
                }
                LineType::CodeBegin => {
                    if code_syntax.is_none() {
                        if let Some(syntax) = self.syntax_set.find_syntax_by_first_line(line) {
                            code_syntax = Some(syntax.clone());
                        }
                    }
                    line_type = LineType::CodeInner;
                    is_code = true;
                }
                LineType::CodeInner => {
                    is_code = true;
                }
            }
        }
        (line_type, code_syntax, is_code)
    }

    fn highlight_line(&self, line: &str, syntax: &SyntaxReference, is_code: bool) -> String {
        let ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        let trimed_line: &str = &line[ws.len()..];
        let mut line_highlighted = None;
        if let Some(theme) = &self.md_theme {
            let mut highlighter = HighlightLines::new(syntax, theme);
            if let Ok(ranges) = highlighter.highlight_line(trimed_line, &self.syntax_set) {
                line_highlighted = Some(format!("{ws}{}", as_terminal_escaped(&ranges)))
            }
        }
        let line = line_highlighted.unwrap_or_else(|| line.into());
        self.wrap_line(line, is_code)
    }

    fn highlight_code_line(&self, line: &str, code_syntax: &Option<SyntaxReference>) -> String {
        if let Some(syntax) = code_syntax {
            self.highlight_line(line, syntax, true)
        } else {
            let line = match self.code_color {
                Some(color) => line.with(color).to_string(),
                None => line.to_string(),
            };
            self.wrap_line(line, true)
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
        if let Some(new_lang) = LANG_MAPS.get(&lang.to_ascii_lowercase()) {
            self.syntax_set.find_syntax_by_name(new_lang)
        } else {
            self.syntax_set
                .find_syntax_by_token(lang)
                .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub highlight: bool,
    pub light_theme: bool,
    pub wrap: Option<String>,
    pub wrap_code: bool,
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
    const TEXT_NO_WRAP_CODE: &str = r#"
To unzip a file in Rust, you can use the `zip` crate. Here's an example code
that shows how to unzip a file:

```rust
use std::fs::File;

fn unzip_file(path: &str, output_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    todo!()
}
```
"#;

    const TEXT_WRAP_ALL: &str = r#"
To unzip a file in Rust, you can use the `zip` crate. Here's an example code
that shows how to unzip a file:

```rust
use std::fs::File;

fn unzip_file(path: &str, output_dir: &str) -> Result<(), Box<dyn
std::error::Error>> {
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
        let options = RenderOptions::default();
        let mut render = MarkdownRender::init(options).unwrap();
        let output = render.render(TEXT);
        assert_eq!(TEXT, output);
    }

    #[test]
    fn no_wrap_code() {
        let options = RenderOptions::default();
        let mut render = MarkdownRender::init(options).unwrap();
        render.wrap_width = Some(80);
        let output = render.render(TEXT);
        assert_eq!(TEXT_NO_WRAP_CODE, output);
    }

    #[test]
    fn wrap_all() {
        let options = RenderOptions {
            wrap_code: true,
            ..Default::default()
        };
        let mut render = MarkdownRender::init(options).unwrap();
        render.wrap_width = Some(80);
        let output = render.render(TEXT);
        assert_eq!(TEXT_WRAP_ALL, output);
    }
}
