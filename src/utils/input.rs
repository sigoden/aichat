use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{stdout, Write};

/// Reads a single character from stdin without requiring Enter
/// Returns the character if it's one of the valid options, or the default if Enter is pressed
pub fn read_single_key(valid_chars: &[char], default: char, prompt: &str) -> Result<char> {
    print!("{prompt}");
    stdout().flush()?;

    enable_raw_mode()?;

    let result = loop {
        if let Ok(Event::Key(KeyEvent {
            code, modifiers, ..
        })) = event::read()
        {
            match code {
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err(anyhow::anyhow!("Interrupted"));
                }
                KeyCode::Char(c) => {
                    if valid_chars.contains(&c) {
                        break Ok(c);
                    }
                    // Invalid character, continue loop
                }
                KeyCode::Enter => {
                    break Ok(default);
                }
                _ => {
                    // Other keys are ignored, continue loop
                }
            }
        }
    };

    disable_raw_mode()?;

    // Print the chosen character and newline for clean output
    if let Ok(chosen) = &result {
        println!("{chosen}");
    }

    result
}
