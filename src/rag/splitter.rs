use super::{RagDocument, RagMetadata};

use std::cmp::Ordering;

pub const DEFAULT_SEPARATES: [&str; 4] = ["\n\n", "\n", " ", ""];
pub const HTML_SEPARATES: [&str; 28] = [
    // First, try to split along HTML tags
    "<body>", "<div>", "<p>", "<br>", "<li>", "<h1>", "<h2>", "<h3>", "<h4>", "<h5>", "<h6>",
    "<span>", "<table>", "<tr>", "<td>", "<th>", "<ul>", "<ol>", "<header>", "<footer>", "<nav>",
    // Head
    "<head>", "<style>", "<script>", "<meta>", "<title>", // Normal type of lines
    " ", "",
];
pub const MARKDOWN_SEPARATES: [&str; 13] = [
    // First, try to split along Markdown headings (starting with level 2)
    "\n## ",
    "\n### ",
    "\n#### ",
    "\n##### ",
    "\n###### ",
    // Note the alternative syntax for headings (below) is not handled here
    // Heading level 2
    // ---------------
    // End of code block
    "```\n\n",
    // Horizontal lines
    "\n\n***\n\n",
    "\n\n---\n\n",
    "\n\n___\n\n",
    // Note that this splitter doesn't handle horizontal lines defined
    // by *three or more* of ***, ---, or ___, but this is not handled
    "\n\n",
    "\n",
    " ",
    "",
];
pub const LATEX_SEPARATES: [&str; 19] = [
    // First, try to split along Latex sections
    "\n\\chapter{",
    "\n\\section{",
    "\n\\subsection{",
    "\n\\subsubsection{",
    // Now split by environments
    "\n\\begin{enumerate}",
    "\n\\begin{itemize}",
    "\n\\begin{description}",
    "\n\\begin{list}",
    "\n\\begin{quote}",
    "\n\\begin{quotation}",
    "\n\\begin{verse}",
    "\n\\begin{verbatim}",
    // Now split by math environments
    "\n\\begin{align}",
    "$$",
    "$",
    // Now split by the normal type of lines
    "\n\n",
    "\n",
    " ",
    "",
];

pub fn autodetect_separator(extension: &str) -> &[&'static str] {
    match extension {
        "md" | "mkd" => &MARKDOWN_SEPARATES,
        "htm" | "html" => &HTML_SEPARATES,
        "tex" => &LATEX_SEPARATES,
        _ => &DEFAULT_SEPARATES,
    }
}

pub struct Splitter {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub separators: Vec<String>,
    pub length_function: Box<dyn Fn(&str) -> usize + Send + Sync>,
}

impl Default for Splitter {
    fn default() -> Self {
        Self {
            chunk_size: 1000,
            chunk_overlap: 20,
            separators: DEFAULT_SEPARATES.iter().map(|v| v.to_string()).collect(),
            length_function: Box::new(|text| text.len()),
        }
    }
}

// Builder pattern for Options struct
impl Splitter {
    pub fn new(chunk_size: usize, chunk_overlap: usize, separators: &[&str]) -> Self {
        Self::default()
            .with_chunk_size(chunk_size)
            .with_chunk_overlap(chunk_overlap)
            .with_separators(separators)
    }

    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    pub fn with_chunk_overlap(mut self, chunk_overlap: usize) -> Self {
        self.chunk_overlap = chunk_overlap;
        self
    }

    pub fn with_separators(mut self, separators: &[&str]) -> Self {
        self.separators = separators.iter().map(|v| v.to_string()).collect();
        self
    }

    #[allow(unused)]
    pub fn with_length_function<F>(mut self, length_function: F) -> Self
    where
        F: Fn(&str) -> usize + Send + Sync + 'static,
    {
        self.length_function = Box::new(length_function);
        self
    }

    pub fn split_documents(
        &self,
        documents: &[RagDocument],
        chunk_header_options: &SplitterChunkHeaderOptions,
    ) -> Vec<RagDocument> {
        let mut texts: Vec<String> = Vec::new();
        let mut metadatas: Vec<RagMetadata> = Vec::new();
        documents.iter().for_each(|d| {
            if !d.page_content.is_empty() {
                texts.push(d.page_content.clone());
                metadatas.push(d.metadata.clone());
            }
        });

        self.create_documents(&texts, &metadatas, chunk_header_options)
    }

    pub fn create_documents(
        &self,
        texts: &[String],
        metadatas: &[RagMetadata],
        chunk_header_options: &SplitterChunkHeaderOptions,
    ) -> Vec<RagDocument> {
        let SplitterChunkHeaderOptions {
            chunk_header,
            chunk_overlap_header,
            append_chunk_overlap_header,
        } = chunk_header_options;

        let mut documents = Vec::new();
        for (i, text) in texts.iter().enumerate() {
            let mut line_counter_index = 1;
            let mut prev_chunk = None;
            let mut index_prev_chunk = None;

            for chunk in self.split_text(text) {
                let mut page_content = chunk_header.clone();

                let index_chunk = {
                    let idx = match index_prev_chunk {
                        Some(v) => v + 1,
                        None => 0,
                    };
                    text[idx..].find(&chunk).map(|i| i + idx).unwrap_or(0)
                };
                if prev_chunk.is_none() {
                    line_counter_index += self.number_of_newlines(text, 0, index_chunk);
                } else {
                    let index_end_prev_chunk: usize = index_prev_chunk.unwrap_or_default()
                        + (self.length_function)(prev_chunk.as_deref().unwrap_or_default());

                    match index_end_prev_chunk.cmp(&index_chunk) {
                        Ordering::Less => {
                            line_counter_index +=
                                self.number_of_newlines(text, index_end_prev_chunk, index_chunk);
                        }
                        Ordering::Greater => {
                            let number =
                                self.number_of_newlines(text, index_chunk, index_end_prev_chunk);
                            line_counter_index = line_counter_index.saturating_sub(number);
                        }
                        Ordering::Equal => {}
                    }

                    if *append_chunk_overlap_header {
                        page_content += chunk_overlap_header;
                    }
                }

                let newlines_count = self.number_of_newlines(&chunk, 0, chunk.len());

                let mut metadata = metadatas[i].clone();
                metadata.insert(
                    "loc".into(),
                    format!(
                        "{}:{}",
                        line_counter_index,
                        line_counter_index + newlines_count
                    ),
                );
                page_content += &chunk;
                documents.push(RagDocument {
                    page_content,
                    metadata,
                });

                line_counter_index += newlines_count;
                prev_chunk = Some(chunk);
                index_prev_chunk = Some(index_chunk);
            }
        }

        documents
    }

    fn number_of_newlines(&self, text: &str, start: usize, end: usize) -> usize {
        text[start..end].matches('\n').count()
    }

    pub fn split_text(&self, text: &str) -> Vec<String> {
        let keep_separator = self
            .separators
            .iter()
            .any(|v| v.chars().any(|v| !v.is_whitespace()));
        self.split_text_impl(text, &self.separators, keep_separator)
    }

    fn split_text_impl(
        &self,
        text: &str,
        separators: &[String],
        keep_separator: bool,
    ) -> Vec<String> {
        let mut final_chunks = Vec::new();

        let mut separator: String = separators.last().cloned().unwrap_or_default();
        let mut new_separators: Vec<String> = vec![];
        for (i, s) in separators.iter().enumerate() {
            if s.is_empty() {
                separator.clone_from(s);
                break;
            }
            if text.contains(s) {
                separator.clone_from(s);
                new_separators = separators[i + 1..].to_vec();
                break;
            }
        }

        // Now that we have the separator, split the text
        let splits = split_on_separator(text, &separator, keep_separator);

        // Now go merging things, recursively splitting longer texts.
        let mut good_splits = Vec::new();
        let _separator = if keep_separator { "" } else { &separator };
        for s in splits {
            if (self.length_function)(s) < self.chunk_size {
                good_splits.push(s.to_string());
            } else {
                if !good_splits.is_empty() {
                    let merged_text = self.merge_splits(&good_splits, _separator);
                    final_chunks.extend(merged_text);
                    good_splits.clear();
                }
                if new_separators.is_empty() {
                    final_chunks.push(s.to_string());
                } else {
                    let other_info = self.split_text_impl(s, &new_separators, keep_separator);
                    final_chunks.extend(other_info);
                }
            }
        }
        if !good_splits.is_empty() {
            let merged_text = self.merge_splits(&good_splits, _separator);
            final_chunks.extend(merged_text);
        }
        final_chunks
    }

    fn merge_splits(&self, splits: &[String], separator: &str) -> Vec<String> {
        let mut docs = Vec::new();
        let mut current_doc = Vec::new();
        let mut total = 0;
        for d in splits {
            let _len = (self.length_function)(d);
            if total + _len + current_doc.len() * separator.len() > self.chunk_size {
                if total > self.chunk_size {
                    // warn!("Warning: Created a chunk of size {}, which is longer than the specified {}", total, self.chunk_size);
                }
                if !current_doc.is_empty() {
                    let doc = self.join_docs(&current_doc, separator);
                    if let Some(doc) = doc {
                        docs.push(doc);
                    }
                    // Keep on popping if:
                    // - we have a larger chunk than in the chunk overlap
                    // - or if we still have any chunks and the length is long
                    while total > self.chunk_overlap
                        || (total + _len + current_doc.len() * separator.len() > self.chunk_size
                            && total > 0)
                    {
                        total -= (self.length_function)(&current_doc[0]);
                        current_doc.remove(0);
                    }
                }
            }
            current_doc.push(d.to_string());
            total += _len;
        }
        let doc = self.join_docs(&current_doc, separator);
        if let Some(doc) = doc {
            docs.push(doc);
        }
        docs
    }

    fn join_docs(&self, docs: &[String], separator: &str) -> Option<String> {
        let text = docs.join(separator).trim().to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }
}

pub struct SplitterChunkHeaderOptions {
    pub chunk_header: String,
    pub chunk_overlap_header: String,
    pub append_chunk_overlap_header: bool,
}

impl Default for SplitterChunkHeaderOptions {
    fn default() -> Self {
        Self {
            chunk_header: "".into(),
            chunk_overlap_header: "(cont'd) ".into(),
            append_chunk_overlap_header: false,
        }
    }
}

impl SplitterChunkHeaderOptions {
    // Set the value of chunk_header
    #[allow(unused)]
    pub fn with_chunk_header(mut self, header: &str) -> Self {
        self.chunk_header = header.to_string();
        self
    }

    // Set the value of chunk_overlap_header
    #[allow(unused)]
    pub fn with_chunk_overlap_header(mut self, overlap_header: &str) -> Self {
        self.chunk_overlap_header = overlap_header.to_string();
        self
    }

    // Set the value of append_chunk_overlap_header
    #[allow(unused)]
    pub fn with_append_chunk_overlap_header(mut self, value: bool) -> Self {
        self.append_chunk_overlap_header = value;
        self
    }
}

fn split_on_separator<'a>(text: &'a str, separator: &str, keep_separator: bool) -> Vec<&'a str> {
    let splits: Vec<&str> = if !separator.is_empty() {
        if keep_separator {
            let mut splits = Vec::new();
            let mut prev_idx = 0;
            let sep_len = separator.len();

            while let Some(idx) = text[prev_idx..].find(separator) {
                splits.push(&text[prev_idx.saturating_sub(sep_len)..prev_idx + idx]);
                prev_idx += idx + sep_len;
            }

            if prev_idx < text.len() {
                splits.push(&text[prev_idx.saturating_sub(sep_len)..]);
            }

            splits
        } else {
            text.split(separator).collect()
        }
    } else {
        text.split("").collect()
    };
    splits.into_iter().filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};

    fn build_metadata(source: &str, loc_from_line: usize, loc_to_line: usize) -> Value {
        json!({
            "source": source,
            "loc": format!("{loc_from_line}:{loc_to_line}"),
        })
    }
    #[test]
    fn test_split_text() {
        let splitter = Splitter {
            chunk_size: 7,
            chunk_overlap: 3,
            separators: vec![" ".into()],
            ..Default::default()
        };
        let output = splitter.split_text("foo bar baz 123");
        assert_eq!(output, vec!["foo bar", "bar baz", "baz 123"]);
    }

    #[test]
    fn test_create_document() {
        let splitter = Splitter::new(3, 0, &[" "]);
        let chunk_header_options = SplitterChunkHeaderOptions::default();
        let mut metadata1 = IndexMap::new();
        metadata1.insert("source".into(), "1".into());
        let mut metadata2 = IndexMap::new();
        metadata2.insert("source".into(), "2".into());
        let output = splitter.create_documents(
            &["foo bar".into(), "baz".into()],
            &[metadata1, metadata2],
            &chunk_header_options,
        );
        let output = json!(output);
        assert_eq!(
            output,
            json!([
                {
                    "page_content": "foo",
                    "metadata": build_metadata("1", 1, 1),
                },
                {
                    "page_content": "bar",
                    "metadata": build_metadata("1", 1, 1),
                },
                {
                    "page_content": "baz",
                    "metadata": build_metadata("2", 1, 1),
                },
            ])
        );
    }

    #[test]
    fn test_chunk_header() {
        let splitter = Splitter::new(3, 0, &[" "]);
        let chunk_header_options = SplitterChunkHeaderOptions::default()
            .with_chunk_header("SOURCE NAME: testing\n-----\n")
            .with_append_chunk_overlap_header(true);
        let mut metadata1 = IndexMap::new();
        metadata1.insert("source".into(), "1".into());
        let mut metadata2 = IndexMap::new();
        metadata2.insert("source".into(), "2".into());
        let output = splitter.create_documents(
            &["foo bar".into(), "baz".into()],
            &[metadata1, metadata2],
            &chunk_header_options,
        );
        let output = json!(output);
        assert_eq!(
            output,
            json!([
                {
                    "page_content": "SOURCE NAME: testing\n-----\nfoo",
                    "metadata": build_metadata("1", 1, 1),
                },
                {
                    "page_content": "SOURCE NAME: testing\n-----\n(cont'd) bar",
                    "metadata": build_metadata("1", 1, 1),
                },
                {
                    "page_content": "SOURCE NAME: testing\n-----\nbaz",
                    "metadata": build_metadata("2", 1, 1),
                },
            ])
        );
    }

    #[test]
    fn test_markdown_splitter() {
        let text = r#"# 🦜️🔗 LangChain

⚡ Building applications with LLMs through composability ⚡

## Quick Install

```bash
# Hopefully this code block isn't split
pip install langchain
```

As an open source project in a rapidly developing field, we are extremely open to contributions."#;
        let splitter = Splitter::new(100, 0, &MARKDOWN_SEPARATES);
        let output = splitter.split_text(text);
        let expected_output = vec![
            "# 🦜️🔗 LangChain\n\n⚡ Building applications with LLMs through composability ⚡",
            "## Quick Install\n\n```bash\n# Hopefully this code block isn't split\npip install langchain",
            "```",
            "As an open source project in a rapidly developing field, we are extremely open to contributions.",
        ];
        assert_eq!(output, expected_output);
    }

    #[test]
    fn test_html_splitter() {
        let text = r#"<!DOCTYPE html>
<html>
  <head>
    <title>🦜️🔗 LangChain</title>
    <style>
      body {
        font-family: Arial, sans-serif;
      }
      h1 {
        color: darkblue;
      }
    </style>
  </head>
  <body>
    <div>
      <h1>🦜️🔗 LangChain</h1>
      <p>⚡ Building applications with LLMs through composability ⚡</p>
    </div>
    <div>
      As an open source project in a rapidly developing field, we are extremely open to contributions.
    </div>
  </body>
</html>"#;
        let splitter = Splitter::new(175, 20, &HTML_SEPARATES);
        let output = splitter.split_text(text);
        let expected_output = vec![
            "<!DOCTYPE html>\n<html>",
            "<head>\n    <title>🦜️🔗 LangChain</title>",
            r#"<style>
      body {
        font-family: Arial, sans-serif;
      }
      h1 {
        color: darkblue;
      }
    </style>
  </head>"#,
            r#"<body>
    <div>
      <h1>🦜️🔗 LangChain</h1>
      <p>⚡ Building applications with LLMs through composability ⚡</p>
    </div>"#,
            r#"<div>
      As an open source project in a rapidly developing field, we are extremely open to contributions.
    </div>
  </body>
</html>"#,
        ];
        assert_eq!(output, expected_output);
    }
}
