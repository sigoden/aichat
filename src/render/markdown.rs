use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;
use syntect::{easy::HighlightLines, parsing::SyntaxReference};

const THEME: &[u8] = include_bytes!("theme.yaml");

pub struct MarkdownRender {
    syntax_set: SyntaxSet,
    theme: Theme,
    md_syntax: SyntaxReference,
    txt_syntax: SyntaxReference,
    code_syntax: SyntaxReference,
    line_type: LineType,
}

impl MarkdownRender {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme: Theme = serde_yaml::from_slice(THEME).unwrap();
        let md_syntax = syntax_set.find_syntax_by_extension("md").unwrap().clone();
        let txt_syntax = syntax_set.find_syntax_by_extension("txt").unwrap().clone();
        let code_syntax = txt_syntax.clone();
        let line_type = LineType::Normal;
        Self {
            syntax_set,
            theme,
            md_syntax,
            code_syntax,
            txt_syntax,
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
                        self.txt_syntax.clone()
                    } else {
                        self.find_syntax(&lang)
                            .cloned()
                            .unwrap_or_else(|| self.txt_syntax.clone())
                    };
                }
                LineType::CodeBegin | LineType::CodeInner => {
                    self.line_type = LineType::CodeEnd;
                    self.code_syntax = self.txt_syntax.clone();
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
                    self.render_line_inner(line, &self.code_syntax)
                }
                LineType::CodeInner => self.render_line_inner(line, &self.code_syntax),
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
                _ => self.render_line_inner(line, &self.code_syntax),
            }
        };

        output.unwrap_or_else(|| line.to_string())
    }

    fn render_line_inner(&self, line: &str, syntax: &SyntaxReference) -> Option<String> {
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let ranges = highlighter.highlight_line(line, &self.syntax_set).ok()?;
        Some(as_24_bit_terminal_escaped(&ranges[..], false))
    }

    fn find_syntax(&self, lang: &str) -> Option<&SyntaxReference> {
        self.syntax_set.find_syntax_by_extension(lang).or_else(|| {
            LANGEGUATE_NAME_EXTS
                .iter()
                .find(|(name, _)| *name == lang.to_lowercase())
                .and_then(|(_, ext)| self.syntax_set.find_syntax_by_extension(ext))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineType {
    Normal,
    CodeBegin,
    CodeInner,
    CodeEnd,
}

const LANGEGUATE_NAME_EXTS: [(&str, &str); 21] = [
    ("asp", "asa"),
    ("actionscript", "as"),
    ("c#", "cs"),
    ("clojure", "clj"),
    ("erlang", "erl"),
    ("haskell", "hs"),
    ("javascript", "js"),
    ("bibtex", "bib"),
    ("latex", "tex"),
    ("tex", "sty"),
    ("ocaml", "ml"),
    ("ocamllex", "mll"),
    ("ocamlyacc", "mly"),
    ("objective-c++", "mm"),
    ("objective-c", "m"),
    ("pascal", "pas"),
    ("perl", "pl"),
    ("python", "py"),
    ("restructuredtext", "rst"),
    ("ruby", "rb"),
    ("rust", "rs"),
];

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
