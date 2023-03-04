use crate::{repl::RenderStreamEvent, utils::dump};
use anyhow::Result;
use crossbeam::channel::Receiver;
use mdcat::{
    push_tty,
    terminal::{TerminalProgram, TerminalSize},
    Environment, ResourceAccess, Settings,
};
use pulldown_cmark::Parser;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use syntect::parsing::SyntaxSet;

pub fn render_stream(
    rx: Receiver<RenderStreamEvent>,
    ctrlc: Arc<AtomicBool>,
    markdown_render: Arc<MarkdownRender>,
) -> Result<()> {
    let mut buffer = String::new();
    let mut line_index = 0;
    loop {
        if ctrlc.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Ok(evt) = rx.try_recv() {
            match evt {
                RenderStreamEvent::Text(text) => {
                    buffer.push_str(&text);
                    if text.contains('\n') {
                        let markdown = markdown_render.render(&buffer)?;
                        let lines: Vec<&str> = markdown.lines().collect();
                        let (_, print_lines) = lines.split_at(line_index);
                        let mut print_lines = print_lines.to_vec();
                        print_lines.pop();
                        if !print_lines.is_empty() {
                            line_index += print_lines.len();
                            dump(print_lines.join("\n").to_string(), 1);
                        }
                    }
                }
                RenderStreamEvent::Done => {
                    let markdown = markdown_render.render(&buffer)?;
                    let tail = markdown
                        .lines()
                        .skip(line_index)
                        .collect::<Vec<&str>>()
                        .join("\n");
                    dump(tail, 2);
                    break;
                }
            }
        }
    }

    Ok(())
}

pub struct MarkdownRender {
    env: Environment,
    settings: Settings,
}

impl MarkdownRender {
    pub fn init() -> Result<Self> {
        let terminal = TerminalProgram::detect();
        let env =
            Environment::for_local_directory(&std::env::current_dir().expect("Working directory"))?;
        let settings = Settings {
            resource_access: ResourceAccess::LocalOnly,
            syntax_set: SyntaxSet::load_defaults_newlines(),
            terminal_capabilities: terminal.capabilities(),
            terminal_size: TerminalSize::default(),
        };
        Ok(Self { env, settings })
    }

    pub fn print(&self, input: &str) -> Result<()> {
        let markdown = self.render(input)?;
        dump(markdown, 0);
        Ok(())
    }

    pub fn render(&self, input: &str) -> Result<String> {
        let source = Parser::new(input);
        let mut sink = Vec::new();
        push_tty(&self.settings, &self.env, &mut sink, source)?;
        Ok(String::from_utf8_lossy(&sink).into())
    }
}
