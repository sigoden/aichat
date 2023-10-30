use super::MarkdownRender;

use crate::print_now;
use crate::repl::{ReplyStreamEvent, SharedAbortSignal};

use anyhow::Result;
use crossbeam::channel::Receiver;
use textwrap::core::display_width;

#[allow(clippy::unnecessary_wraps, clippy::module_name_repetitions)]
pub fn cmd_render_stream(
    rx: &Receiver<ReplyStreamEvent>,
    render: &mut MarkdownRender,
    abort: &SharedAbortSignal,
) -> Result<()> {
    let mut buffer = String::new();
    let mut col = 0;
    loop {
        if abort.aborted() {
            return Ok(());
        }
        if let Ok(evt) = rx.try_recv() {
            match evt {
                ReplyStreamEvent::Text(text) => {
                    if text.contains('\n') {
                        let text = format!("{buffer}{text}");
                        let (head, tail) = split_line_tail(&text);
                        buffer = tail.to_string();
                        let input = format!("{}{head}", spaces(col));
                        let output = render.render(&input);
                        print_now!("{}\n", &output[col..]);
                        col = 0;
                    } else {
                        buffer = format!("{buffer}{text}");
                        if !(render.is_code()
                            || buffer.len() < 40
                            || buffer.starts_with('#')
                            || buffer.starts_with('>')
                            || buffer.starts_with('|'))
                        {
                            if let Some((head, remain)) = split_line_sematic(&buffer) {
                                buffer = remain;
                                let input = format!("{}{head}", spaces(col));
                                let output = render.render(&input);
                                let output = &output[col..];
                                let (_, tail) = split_line_tail(output);
                                if output.contains('\n') {
                                    col = display_width(tail);
                                } else {
                                    col += display_width(output);
                                }
                                print_now!("{}", output);
                            }
                        }
                    }
                }
                ReplyStreamEvent::Done => {
                    let input = format!("{}{buffer}", spaces(col));
                    print_now!("{}\n", render.render(&input));
                    break;
                }
            }
        }
    }
    Ok(())
}

fn split_line_sematic(text: &str) -> Option<(String, String)> {
    let mut balance: Vec<Kind> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
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
        do_balance(&mut balance, &chars[index..=index]);
        index += 1;
    }

    None
}

pub(crate) fn split_line_tail(text: &str) -> (&str, &str) {
    if let Some((head, tail)) = text.rsplit_once('\n') {
        (head, tail)
    } else {
        ("", text)
    }
}

fn spaces(n: usize) -> String {
    " ".repeat(n)
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
                '(' => Self::ParentheseStart,
                ')' => Self::ParentheseEnd,
                '[' => Self::BracketStart,
                ']' => Self::BracketEnd,
                '*' => Self::Asterisk,
                '\'' => Self::SingleQuota,
                '"' => Self::DoubleQuota,
                '~' => Self::Tilde,
                '`' => Self::Backtick,
                _ => return None,
            },
            2 if chars[0] == chars[1] => match chars[0] {
                '*' => Self::Asterisk2,
                '~' => Self::Tilde2,
                _ => return None,
            },
            3 => {
                if chars == ['`', '`', '`'] {
                    Self::Backtick3
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
    Kind::from_chars(chars).map_or(false, |kind| {
        let last = balance.last();
        match (kind, last) {
            (Kind::ParentheseEnd, Some(&Kind::ParentheseStart))
            | (Kind::BracketEnd, Some(&Kind::BracketStart))
            | (Kind::Asterisk, Some(&Kind::Asterisk))
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
            (
                Kind::ParentheseStart
                | Kind::BracketStart
                | Kind::Asterisk
                | Kind::Asterisk2
                | Kind::SingleQuota
                | Kind::DoubleQuota
                | Kind::Tilde
                | Kind::Tilde2
                | Kind::Backtick
                | Kind::Backtick3,
                _,
            ) => {
                balance.push(kind);
                true
            }
            _ => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_split_line {
        ($a:literal, $b:literal, true) => {
            assert_eq!(
                split_line_sematic(&format!("{}{}", $a, $b)),
                Some(($a.into(), $b.into()))
            );
        };
        ($a:literal, $b:literal, false) => {
            assert_eq!(split_line_sematic(&format!("{}{}", $a, $b)), None);
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
