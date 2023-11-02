pub fn split_line_sematic(text: &str) -> Option<(String, String)> {
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

pub fn split_line_tail(text: &str) -> (&str, &str) {
    if let Some((head, tail)) = text.rsplit_once('\n') {
        (head, tail)
    } else {
        ("", text)
    }
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
            "Wikipedia is a free online encyclopedia,",
            " that anyone can edit,",
            true
        );
        assert_split_line!(
            "Wikipedia is a free online encyclopedia.",
            " that anyone can edit,",
            true
        );
        assert_split_line!("床前明月光，", "疑是地上霜。", true);
        assert_split_line!("床前明月光。", "疑是地上霜。", true);
        assert_split_line!("床前明月光；", "疑是地上霜。", true);
        assert_split_line!(
            "Wikipedia is (a free online encyclopedia).",
            " that anyone can edit.",
            true
        );
        assert_split_line!(
            "Wikipedia is a free online `encyclopedia,",
            " that` anyone can edit.",
            false
        );
        assert_split_line!(
            "Wikipedia is a free online ```encyclopedia,",
            " that``` anyone can edit.",
            false
        );
        assert_split_line!(
            "Wikipedia is a free online *encyclopedia,",
            " that* anyone can edit.",
            false
        );
        assert_split_line!(
            "Wikipedia is a free online **encyclopedia,",
            " that** anyone can edit.",
            false
        );
        assert_split_line!(
            "Wikipedia is a free online ~encyclopedia,",
            " that~ anyone can edit.",
            false
        );
        assert_split_line!(
            "Wikipedia is a free online ~~encyclopedia,",
            " that~~ anyone can edit.",
            false
        );
        assert_split_line!(
            "Wikipedia is a free online ``encyclopedia,",
            " that`` anyone can edit.",
            true
        );
        assert_split_line!(
            "Wikipedia is a free online \"encyclopedia,",
            " that\" anyone can edit.",
            false
        );
        assert_split_line!(
            "Wikipedia is a free online 'encyclopedia,",
            " that' anyone can edit.",
            false
        );
        assert_split_line!(
            "Wikipedia is a free online encyclopedia.",
            "that anyone can edit.",
            false
        );
    }
}
