#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub client: String,
    pub name: String,
    pub max_tokens: Option<usize>,
    pub index: usize,
}

impl Default for ModelInfo {
    fn default() -> Self {
        ModelInfo::new("", "", None, 0)
    }
}

impl ModelInfo {
    pub fn new(client: &str, name: &str, max_tokens: Option<usize>, index: usize) -> Self {
        Self {
            client: client.into(),
            name: name.into(),
            max_tokens,
            index,
        }
    }
    pub fn stringify(&self) -> String {
        format!("{}:{}", self.client, self.name)
    }
}
