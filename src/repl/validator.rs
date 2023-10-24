use reedline::{ValidationResult, Validator};

/// A default validator which checks for mismatched quotes and brackets
#[allow(clippy::module_name_repetitions)]
pub struct ReplValidator;

impl Validator for ReplValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if line.ends_with('\\') {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}
