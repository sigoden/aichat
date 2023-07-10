use reedline::{ValidationResult, Validator};

/// A default validator which checks for mismatched quotes and brackets
#[allow(clippy::module_name_repetitions)]
pub struct ReplValidator;

impl Validator for ReplValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if incomplete_brackets(line) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

fn incomplete_brackets(line: &str) -> bool {
    let mut balance: Vec<char> = Vec::new();
    let mut symbol = None;
    for c in line.chars() {
        match symbol {
            Some(s) => match (s, c) {
                ('{', '}') | ('(', ')') => {
                    balance.pop();
                }
                _ if s == c => {
                    balance.push(c);
                }
                _ => {}
            },
            None => match c {
                '{' | '(' => {
                    balance.push(c);
                    symbol = Some(c);
                }
                _ => {}
            },
        }
    }

    !balance.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incomplete_brackets() {
        assert!(incomplete_brackets("{"));
        assert!(incomplete_brackets("("));
        assert!(!incomplete_brackets("{}"));
        assert!(!incomplete_brackets("()"));
        assert!(!incomplete_brackets("{ab\nc}"));
        assert!(!incomplete_brackets("(ab\nc)"));
        assert!(!incomplete_brackets("{[}"));
        assert!(!incomplete_brackets("{{{{{}}}}}"));
        assert!(incomplete_brackets("{{}"));
    }
}
