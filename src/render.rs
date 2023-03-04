use anyhow::Result;
use crossbeam::channel::Receiver;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    style::{self, Color},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use mdcat::{
    push_tty,
    terminal::{TerminalProgram, TerminalSize},
    Environment, ResourceAccess, Settings,
};
use pulldown_cmark::Parser;
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use syntect::parsing::SyntaxSet;

use crate::repl::{dump, RenderStreamEvent};

pub fn render_stream(
    rx: Receiver<RenderStreamEvent>,
    ctrlc: Arc<AtomicBool>,
    markdown_render: Arc<MarkdownRender>,
) -> Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = tui::backend::CrosstermBackend::new(stdout);
    let mut terminal = tui::Terminal::new(backend)?;

    // create app and run it
    let app = render_stream_tui::App::new(ctrlc, markdown_render);
    let res = render_stream_tui::run(&mut terminal, app, rx);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

mod render_stream_tui {
    use super::*;
    use ansi_to_tui::IntoText;
    use crossterm::event::MouseEventKind;
    use tui::{
        backend::Backend,
        layout::{Constraint, Direction, Layout},
        style::Style,
        widgets::{List, ListItem, ListState},
        Frame, Terminal,
    };

    pub fn run<B: Backend>(
        terminal: &mut Terminal<B>,
        mut app: App,
        rx: Receiver<RenderStreamEvent>,
    ) -> Result<()> {
        let mut last_tick = Instant::now();
        let tick_rate = Duration::from_millis(250);
        let mut count_evt = 0;
        let mut count_done = 0;

        loop {
            if app.ctrlc.load(Ordering::SeqCst) {
                return Ok(());
            }

            if let Ok(evt) = rx.try_recv() {
                count_evt += 1;
                app.handle(evt)?;
                if count_evt <= 16 {
                    continue;
                } else {
                    count_evt = 0;
                }
            }

            terminal.draw(|f| ui(f, &mut app))?;

            if app.no_interrupt && app.done {
                count_done += 1;
                if count_done >= 5 {
                    return Ok(());
                }
            }

            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));
            if crossterm::event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) => {
                        app.no_interrupt = false;
                        match key.code {
                            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                                app.quit();
                                return Ok(());
                            }
                            KeyCode::Down => app.next(),
                            KeyCode::Up => app.previous(),
                            _ => {}
                        }
                    }
                    Event::Mouse(ev) => {
                        app.no_interrupt = false;
                        match ev.kind {
                            MouseEventKind::ScrollDown => app.next(),
                            MouseEventKind::ScrollUp => app.previous(),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            if last_tick.elapsed() >= tick_rate {
                app.on_tick();
                last_tick = Instant::now();
            }
        }
    }

    pub struct App {
        buffer: String,
        items: Vec<String>,
        list_state: ListState,
        done: bool,
        ctrlc: Arc<AtomicBool>,
        markdown_render: Arc<MarkdownRender>,
        no_interrupt: bool,
        num_rows: usize,
        entry_index: usize,
    }

    impl App {
        pub fn new(ctrlc: Arc<AtomicBool>, markdown_render: Arc<MarkdownRender>) -> Self {
            Self {
                buffer: String::new(),
                ctrlc,
                done: false,
                markdown_render,
                num_rows: 0,
                no_interrupt: true,
                entry_index: 0,
                items: vec![],
                list_state: ListState::default(),
            }
        }

        pub fn handle(&mut self, evt: RenderStreamEvent) -> Result<()> {
            match evt {
                RenderStreamEvent::Start(question) => {
                    let mut buf = Vec::with_capacity(8);
                    execute!(
                        buf,
                        style::SetForegroundColor(Color::Cyan),
                        style::Print("〉"),
                        style::ResetColor
                    )?;
                    let indicator = String::from_utf8_lossy(&buf);
                    self.buffer.push_str(&format!("{indicator}{question}\n"));
                }
                RenderStreamEvent::Text(text) => {
                    self.buffer.push_str(&text);
                }
                RenderStreamEvent::Done => {
                    self.done = true;
                }
            }

            let markdown = self.markdown_render.render(&self.buffer)?;
            self.items = markdown.split('\n').map(|v| v.to_string()).collect();
            if self.no_interrupt {
                self.end();
            }

            Ok(())
        }

        pub fn quit(&mut self) {
            self.ctrlc.store(true, Ordering::SeqCst);
        }

        pub fn set_rows(&mut self, rows: u16) {
            self.num_rows = rows as usize;
        }

        pub fn next(&mut self) {
            let index = if self.entry_index < self.num_rows {
                self.num_rows.min(self.items.len() - 1)
            } else {
                self.entry_index + 1
            };
            self.entry_index = index;
            self.list_state.select(Some(index));
        }

        pub fn previous(&mut self) {
            let index = self
                .entry_index
                .saturating_sub(1)
                .min(self.items.len().saturating_sub(self.num_rows + 1));
            self.entry_index = index;
            self.list_state.select(Some(index));
        }

        pub fn end(&mut self) {
            let len = self.items.len();
            self.entry_index = if len < self.num_rows {
                0
            } else {
                len - self.num_rows
            };
            self.list_state.select(Some(len - 1))
        }

        pub fn on_tick(&mut self) {}
    }

    fn ui<B: Backend>(f: &mut Frame<B>, app: &mut App) {
        // Create two chunks with equal horizontal screen space
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100)].as_ref())
            .split(f.size());

        let items: Vec<ListItem> = app
            .items
            .iter()
            .map(|line| {
                let text = line.into_text().unwrap_or_default();
                ListItem::new(text)
            })
            .collect();

        app.set_rows(chunks[0].height);
        let items = List::new(items).highlight_style(Style::default());
        f.render_stateful_widget(items, chunks[0], &mut app.list_state);
    }
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
