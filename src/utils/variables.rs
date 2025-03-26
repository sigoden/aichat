use super::*;
use fancy_regex::{Captures, Regex};
use std::sync::LazyLock;

pub static RE_VARIABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{(\w+)\}\}").unwrap());
pub fn interpolate_variables(text: &mut String) {
    *text = RE_VARIABLE
        .replace_all(text, |caps: &Captures<'_>| {
            let key = &caps[1];
            match key {
                "__os__" => env::consts::OS.to_string(),
                "__os_distro__" => {
                    let info = os_info::get();
                    if env::consts::OS == "linux" {
                        format!("{info} (linux)")
                    } else {
                        info.to_string()
                    }
                }
                "__os_family__" => env::consts::FAMILY.to_string(),
                "__arch__" => env::consts::ARCH.to_string(),
                "__shell__" => SHELL.name.clone(),
                "__locale__" => sys_locale::get_locale().unwrap_or_default(),
                "__now__" => now(),
                "__cwd__" => env::current_dir()
                    .map(|v| v.display().to_string())
                    .unwrap_or_default(),
                _ => format!("{{{{{}}}}}", key),
            }
        })
        .to_string();
}
