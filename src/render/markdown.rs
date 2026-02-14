use syntect::highlighting::Theme;

#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub theme: Option<Theme>,
    pub wrap: Option<String>,
    pub truecolor: bool,
}

impl RenderOptions {
    pub(crate) fn new(theme: Option<Theme>, wrap: Option<String>, truecolor: bool) -> Self {
        Self {
            theme,
            wrap,
            truecolor,
        }
    }
}
