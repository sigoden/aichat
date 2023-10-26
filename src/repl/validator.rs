use super::EDIT_RE;

use reedline::{ValidationResult, Validator};

/// A default validator which checks for mismatched quotes and brackets
pub struct ReplValidator;

impl Validator for ReplValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if let Ok(true) = EDIT_RE.is_match(line) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}
