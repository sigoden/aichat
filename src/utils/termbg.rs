//! Terminal background color detection
/// Fork from https://github.com/dalance/termbg/blob/v0.4.3/src/lib.rs
use anyhow::{anyhow, Error, Result};
use crossterm::terminal;
use std::env;
use std::io::{self, Read, Write};
use std::time::Duration;

/// Terminal
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Terminal {
    Screen,
    Tmux,
    XtermCompatible,
    VSCode,
    Emacs,
}

/// 16bit RGB color
#[derive(Copy, Clone, Debug)]
pub struct Rgb {
    pub r: u16,
    pub g: u16,
    pub b: u16,
}

/// Background theme
#[derive(Copy, Clone, Debug)]
pub enum Theme {
    Light,
    Dark,
}

/// get detected termnial
pub fn terminal() -> Terminal {
    if let Ok(term_program) = env::var("TERM_PROGRAM") {
        if term_program == "vscode" {
            return Terminal::VSCode;
        }
    }

    if env::var("INSIDE_EMACS").is_ok() {
        return Terminal::Emacs;
    }

    if env::var("TMUX").is_ok() {
        Terminal::Tmux
    } else {
        let is_screen = if let Ok(term) = env::var("TERM") {
            term.starts_with("screen")
        } else {
            false
        };
        if is_screen {
            Terminal::Screen
        } else {
            Terminal::XtermCompatible
        }
    }
}

/// get background color by `RGB`
pub fn rgb(timeout: Duration) -> Result<Rgb> {
    let term = terminal();
    let rgb = match term {
        Terminal::VSCode => Err(unsupported_err()),
        Terminal::Emacs => Err(unsupported_err()),
        _ => from_xterm(term, timeout),
    };
    check_rgb(rgb)
}

/// get background color by `Theme`
pub fn theme(timeout: Duration) -> Result<Theme> {
    let rgb = rgb(timeout)?;

    // ITU-R BT.601
    let y = rgb.r as f64 * 0.299 + rgb.g as f64 * 0.587 + rgb.b as f64 * 0.114;

    if y > 32768.0 {
        Ok(Theme::Light)
    } else {
        Ok(Theme::Dark)
    }
}

fn from_xterm(term: Terminal, timeout: Duration) -> Result<Rgb> {
    // Query by XTerm control sequence
    let query = if term == Terminal::Tmux {
        "\x1bPtmux;\x1b\x1b]11;?\x07\x1b\\\x03"
    } else if term == Terminal::Screen {
        "\x1bP\x1b]11;?\x07\x1b\\\x03"
    } else {
        "\x1b]11;?\x1b\\"
    };

    let mut stderr = io::stderr();
    terminal::enable_raw_mode()?;
    write!(stderr, "{}", query)?;
    stderr.flush()?;

    let mut stdin = io::stdin();

    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let mut buf = [0; 1];
        let mut start = false;
        loop {
            let _ = stdin.read_exact(&mut buf);
            // response terminated by BEL(0x7)
            if start && (buf[0] == 0x7) {
                break;
            }
            // response terminated by ST(0x1b 0x5c)
            if start && (buf[0] == 0x1b) {
                // consume last 0x5c
                let _ = stdin.read_exact(&mut buf);
                debug_assert_eq!(buf[0], 0x5c);
                break;
            }
            if start {
                buffer.push(buf[0]);
            }
            if buf[0] == b':' {
                start = true;
            }
        }
        // Ignore send error because timeout may be occured
        let _ = tx.send(buffer);
    });

    let buffer = rx.recv_timeout(timeout);

    terminal::disable_raw_mode()?;

    let buffer = buffer?;

    let s = String::from_utf8_lossy(&buffer);
    let (r, g, b) = decode_x11_color(&s)?;
    Ok(Rgb { r, g, b })
}

fn decode_x11_color(s: &str) -> Result<(u16, u16, u16)> {
    fn decode_hex(s: &str) -> Result<u16> {
        let len = s.len() as u32;
        let mut ret = u16::from_str_radix(s, 16).map_err(|_| parse_err(s))?;
        ret <<= (4 - len) * 4;
        Ok(ret)
    }

    let rgb: Vec<_> = s.split('/').collect();

    let r = rgb.first().ok_or_else(|| parse_err(s))?;
    let g = rgb.get(1).ok_or_else(|| parse_err(s))?;
    let b = rgb.get(2).ok_or_else(|| parse_err(s))?;
    let r = decode_hex(r)?;
    let g = decode_hex(g)?;
    let b = decode_hex(b)?;

    Ok((r, g, b))
}

fn check_rgb(prev: Result<Rgb>) -> Result<Rgb> {
    if prev.is_ok() {
        return prev;
    }
    let fallback = from_env_colorfgbg();
    if fallback.is_ok() {
        return fallback;
    }
    prev
}

fn from_env_colorfgbg() -> Result<Rgb> {
    let var = env::var("COLORFGBG").map_err(|_| unsupported_err())?;
    let fgbg: Vec<_> = var.split(';').collect();
    let bg = fgbg.get(1).ok_or(unsupported_err())?;
    let bg = bg.parse::<u8>().map_err(|_| parse_err(&var))?;

    // rxvt default color table
    let (r, g, b) = match bg {
        // black
        0 => (0, 0, 0),
        // red
        1 => (205, 0, 0),
        // green
        2 => (0, 205, 0),
        // yellow
        3 => (205, 205, 0),
        // blue
        4 => (0, 0, 238),
        // magenta
        5 => (205, 0, 205),

        // cyan
        6 => (0, 205, 205),
        // white
        7 => (229, 229, 229),
        // bright black
        8 => (127, 127, 127),
        // bright red
        9 => (255, 0, 0),
        // bright green
        10 => (0, 255, 0),
        // bright yellow
        11 => (255, 255, 0),
        // bright blue
        12 => (92, 92, 255),
        // bright magenta
        13 => (255, 0, 255),
        // bright cyan
        14 => (0, 255, 255),

        // bright white
        15 => (255, 255, 255),
        _ => (0, 0, 0),
    };

    Ok(Rgb {
        r: r * 256,
        g: g * 256,
        b: b * 256,
    })
}

fn unsupported_err() -> Error {
    anyhow!("Unsupported terminal")
}

fn parse_err(value: &str) -> Error {
    anyhow!("Failed to parse {value}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_x11_color() {
        let s = "0000/0000/0000";
        assert_eq!((0, 0, 0), decode_x11_color(s).unwrap());

        let s = "1111/2222/3333";
        assert_eq!((0x1111, 0x2222, 0x3333), decode_x11_color(s).unwrap());

        let s = "111/222/333";
        assert_eq!((0x1110, 0x2220, 0x3330), decode_x11_color(s).unwrap());

        let s = "11/22/33";
        assert_eq!((0x1100, 0x2200, 0x3300), decode_x11_color(s).unwrap());

        let s = "1/2/3";
        assert_eq!((0x1000, 0x2000, 0x3000), decode_x11_color(s).unwrap());
    }
}
