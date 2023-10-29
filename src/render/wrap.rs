use std::borrow::Cow;

#[derive(Debug, Clone, Copy)]
pub enum Wrap {
    No,
    Width(u16),
}

impl Wrap {
    pub fn new(text_width: Option<u16>) -> Self {
        if let Some(v) = text_width {
            if let Ok((cols, _)) = crossterm::terminal::size() {
                if v == 0 {
                    return Wrap::Width(cols);
                } else {
                    return Wrap::Width(cols.min(v));
                }
            }
        }
        Wrap::No
    }

    pub fn wrap<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if let Wrap::Width(width) = self {
            let out = textwrap::wrap(text, *width as usize).join("\n");
            Cow::Owned(out)
        } else {
            Cow::Borrowed(text)
        }
    }
}
