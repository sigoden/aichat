use anyhow::{anyhow, Result};
use crossterm::terminal;
use streamdown_config::ComputedStyle;
use streamdown_parser::Parser;
use streamdown_plugin::{PluginAction, PluginManager};
use streamdown_render::{RenderState, RenderStyle, Renderer, Theme};

pub struct MarkdownRender {
    parser: Parser,
    render_state: RenderState,
    plugin_manager: PluginManager,
    computed_style: ComputedStyle,
    width: usize,
    light_theme: bool,
    custom_style: Option<RenderStyle>,
    custom_theme: Option<Theme>,
}

impl MarkdownRender {
    pub fn init(options: RenderOptions) -> Result<Self> {
        let terminal_cols = terminal::size()
            .map(|(c, _)| c as usize)
            .unwrap_or(80);
        let width = match options.wrap.as_deref() {
            Some(v) if v != "auto" => {
                let w = v
                    .parse::<usize>()
                    .map_err(|_| anyhow!("Invalid wrap value: '{v}'"))?;
                terminal_cols.min(w)
            }
            _ => terminal_cols,
        };
        Ok(Self {
            parser: Parser::new(),
            render_state: RenderState::default(),
            plugin_manager: PluginManager::with_builtins(),
            computed_style: ComputedStyle::default(),
            width,
            light_theme: options.light_theme,
            custom_style: options.custom_style,
            custom_theme: options.custom_theme,
        })
    }

    pub fn render(&mut self, text: &str) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        self.render_inner(lines.into_iter(), true)
    }

    pub fn render_line(&mut self, line: &str) -> String {
        if line.is_empty() {
            return String::new();
        }
        let parser_snapshot = self.parser.clone();
        let state_snapshot = self.render_state.clone();
        let result = self.render_inner(std::iter::once(line), false);
        self.parser = parser_snapshot;
        self.render_state = state_snapshot;
        result
    }

    fn render_inner<'a>(
        &mut self,
        lines: impl Iterator<Item = &'a str>,
        use_plugins: bool,
    ) -> String {
        let mut output = Vec::new();
        {
            let mut renderer = self.new_renderer(&mut output);
            for line in lines {
                if use_plugins {
                    match self.plugin_manager.process_line(
                        line,
                        self.parser.state(),
                        &self.computed_style,
                    ) {
                        Some(PluginAction::Output(plugin_lines)) => {
                            for pl in &plugin_lines {
                                for event in self.parser.parse_line(pl) {
                                    let _ = renderer.render_event(&event);
                                }
                            }
                            continue;
                        }
                        Some(PluginAction::Rewrite(rewritten)) => {
                            for event in self.parser.parse_line(&rewritten) {
                                let _ = renderer.render_event(&event);
                            }
                            continue;
                        }
                        None => {}
                    }
                }
                for event in self.parser.parse_line(line) {
                    let _ = renderer.render_event(&event);
                }
            }
            self.render_state = renderer.save_state();
        }
        let mut s = String::from_utf8(output).unwrap_or_default();
        if s.ends_with('\n') {
            s.truncate(s.len() - 1);
        }
        s
    }

    pub fn flush(&mut self) -> String {
        let flushed = self.plugin_manager.flush();
        if flushed.is_empty() {
            return String::new();
        }
        let mut output = Vec::new();
        {
            let mut renderer = self.new_renderer(&mut output);
            for line in &flushed {
                for event in self.parser.parse_line(line) {
                    let _ = renderer.render_event(&event);
                }
            }
            self.render_state = renderer.save_state();
        }
        let mut s = String::from_utf8(output).unwrap_or_default();
        if s.ends_with('\n') {
            s.truncate(s.len() - 1);
        }
        s
    }

    fn new_renderer<'a>(&self, output: &'a mut Vec<u8>) -> Renderer<&'a mut Vec<u8>> {
        let mut renderer = Renderer::new(output, self.width);
        if let Some(style) = self.custom_style.clone() {
            renderer.set_style(style);
        }
        if let Some(theme) = self.custom_theme.clone() {
            renderer.set_custom_theme(theme);
        } else if self.light_theme {
            renderer.set_theme("base16-ocean.light");
        }
        renderer.set_pretty_pad(true);
        renderer.restore_state(self.render_state.clone());
        renderer
    }
}

#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub wrap: Option<String>,
    pub light_theme: bool,
    pub custom_style: Option<RenderStyle>,
    pub custom_theme: Option<Theme>,
}

impl RenderOptions {
    pub(crate) fn new(wrap: Option<String>, light_theme: bool, custom_style: Option<RenderStyle>, custom_theme: Option<Theme>) -> Self {
        Self { wrap, light_theme, custom_style, custom_theme }
    }
}
