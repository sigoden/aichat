use super::MarkdownRender;
use crate::repl::{ReplyStreamEvent, SharedAbortSignal};
use crate::utils::dump;

use anyhow::Result;
use crossbeam::channel::Receiver;

pub fn cmd_render_stream(rx: Receiver<ReplyStreamEvent>, abort: SharedAbortSignal) -> Result<()> {
    let mut buffer = String::new();
    let mut markdown_render = MarkdownRender::new();
    loop {
        if abort.aborted() {
            return Ok(());
        }
        if let Ok(evt) = rx.try_recv() {
            match evt {
                ReplyStreamEvent::Text(text) => {
                    if text.contains('\n') {
                        let text = format!("{buffer}{text}");
                        let mut lines: Vec<&str> = text.split('\n').collect();
                        buffer = lines.pop().unwrap_or_default().to_string();
                        let output = lines.join("\n");
                        dump(markdown_render.render(&output), 1);
                    } else {
                        buffer = format!("{buffer}{text}");
                        if !(markdown_render.is_code_block()
                            || buffer.len() < 60
                            || buffer.starts_with('#')
                            || buffer.starts_with('>')
                            || buffer.starts_with('|'))
                        {
                            if let Some((output, remain)) = split_line(&buffer) {
                                dump(markdown_render.render_line_stateless(&output), 0);
                                buffer = remain
                            }
                        }
                    }
                }
                ReplyStreamEvent::Done => {
                    let output = markdown_render.render(&buffer);
                    dump(output, 2);
                    break;
                }
            }
        }
    }
    Ok(())
}

fn split_line(line: &str) -> Option<(String, String)> {
    let mut balance: Vec<Kind> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    let len = chars.len();
    while index < len - 1 {
        let ch = chars[index];
        if balance.is_empty()
            && ((matches!(ch, ',' | '.' | ';') && chars[index + 1].is_whitespace())
                || matches!(ch, '，' | '。' | '；'))
        {
            let (output, remain) = chars.split_at(index + 1);
            return Some((output.iter().collect(), remain.iter().collect()));
        }
        if index + 2 < len && do_balance(&mut balance, &chars[index..=index + 2]) {
            index += 3;
            continue;
        }
        if do_balance(&mut balance, &chars[index..=index + 1]) {
            index += 2;
            continue;
        }
        do_balance(&mut balance, &chars[index..index + 1]);
        index += 1
    }

    None
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Kind {
    ParentheseStart,
    ParentheseEnd,
    BracketStart,
    BracketEnd,
    Asterisk,
    Asterisk2,
    SingleQuota,
    DoubleQuota,
    Tilde,
    Tilde2,
    Backtick,
    Backtick3,
}

impl Kind {
    fn from_chars(chars: &[char]) -> Option<Self> {
        let kind = match chars.len() {
            1 => match chars[0] {
                '(' => Kind::ParentheseStart,
                ')' => Kind::ParentheseEnd,
                '[' => Kind::BracketStart,
                ']' => Kind::BracketEnd,
                '*' => Kind::Asterisk,
                '\'' => Kind::SingleQuota,
                '"' => Kind::DoubleQuota,
                '~' => Kind::Tilde,
                '`' => Kind::Backtick,
                _ => return None,
            },
            2 if chars[0] == chars[1] => match chars[0] {
                '*' => Kind::Asterisk2,
                '~' => Kind::Tilde2,
                _ => return None,
            },
            3 => {
                if chars == ['`', '`', '`'] {
                    Kind::Backtick3
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        Some(kind)
    }
}

fn do_balance(balance: &mut Vec<Kind>, chars: &[char]) -> bool {
    if let Some(kind) = Kind::from_chars(chars) {
        let last = balance.last();
        match (kind, last) {
            (Kind::ParentheseStart | Kind::BracketStart, _) => {
                balance.push(kind);
                true
            }
            (Kind::ParentheseEnd, Some(&Kind::ParentheseStart)) => {
                balance.pop();
                true
            }
            (Kind::BracketEnd, Some(&Kind::BracketStart)) => {
                balance.pop();
                true
            }
            (Kind::Asterisk, Some(&Kind::Asterisk))
            | (Kind::Asterisk2, Some(&Kind::Asterisk2))
            | (Kind::SingleQuota, Some(&Kind::SingleQuota))
            | (Kind::DoubleQuota, Some(&Kind::DoubleQuota))
            | (Kind::Tilde, Some(&Kind::Tilde))
            | (Kind::Tilde2, Some(&Kind::Tilde2))
            | (Kind::Backtick, Some(&Kind::Backtick))
            | (Kind::Backtick3, Some(&Kind::Backtick3)) => {
                balance.pop();
                true
            }
            (Kind::Asterisk, _)
            | (Kind::Asterisk2, _)
            | (Kind::SingleQuota, _)
            | (Kind::DoubleQuota, _)
            | (Kind::Tilde, _)
            | (Kind::Tilde2, _)
            | (Kind::Backtick, _)
            | (Kind::Backtick3, _) => {
                balance.push(kind);
                true
            }
            _ => false,
        }
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_split_line {
        ($a:literal, $b:literal, true) => {
            assert_eq!(
                split_line(&format!("{}{}", $a, $b)),
                Some(($a.into(), $b.into()))
            );
        };
        ($a:literal, $b:literal, false) => {
            assert_eq!(split_line(&format!("{}{}", $a, $b)), None);
        };
    }

    #[test]
    fn test_split_line() {
        assert_split_line!(
            "Lorem ipsum dolor sit amet,",
            " consectetur adipiscing elit.",
            true
        );
        assert_split_line!(
            "Lorem ipsum dolor sit amet.",
            " consectetur adipiscing elit.",
            true
        );
        assert_split_line!("黃更室幼許刀知，", "波食小午足田世根候法。", true);
        assert_split_line!("黃更室幼許刀知。", "波食小午足田世根候法。", true);
        assert_split_line!("黃更室幼許刀知；", "波食小午足田世根候法。", true);
        assert_split_line!(
            "Lorem ipsum (dolor sit amet).",
            " consectetur adipiscing elit.",
            true
        );
        assert_split_line!(
            "Lorem ipsum dolor sit `amet,",
            " consectetur` adipiscing elit.",
            false
        );
        assert_split_line!(
            "Lorem ipsum dolor sit ```amet,",
            " consectetur``` adipiscing elit.",
            false
        );
        assert_split_line!(
            "Lorem ipsum dolor sit *amet,",
            " consectetur* adipiscing elit.",
            false
        );
        assert_split_line!(
            "Lorem ipsum dolor sit **amet,",
            " consectetur** adipiscing elit.",
            false
        );
        assert_split_line!(
            "Lorem ipsum dolor sit ~amet,",
            " consectetur~ adipiscing elit.",
            false
        );
        assert_split_line!(
            "Lorem ipsum dolor sit ~~amet,",
            " consectetur~~ adipiscing elit.",
            false
        );
        assert_split_line!(
            "Lorem ipsum dolor sit ``amet,",
            " consectetur`` adipiscing elit.",
            true
        );
        assert_split_line!(
            "Lorem ipsum dolor sit \"amet,",
            " consectetur\" adipiscing elit.",
            false
        );
        assert_split_line!(
            "Lorem ipsum dolor sit 'amet,",
            " consectetur' adipiscing elit.",
            false
        );
        assert_split_line!(
            "Lorem ipsum dolor sit amet.",
            "consectetur adipiscing elit.",
            false
        );
    }
}
