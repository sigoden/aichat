use std::{cell::RefCell, rc::Rc};

use html_to_markdown::{markdown, TagHandler};

pub fn html_to_md(html: &str) -> String {
    let mut handlers: Vec<TagHandler> = vec![
        Rc::new(RefCell::new(markdown::ParagraphHandler)),
        Rc::new(RefCell::new(markdown::HeadingHandler)),
        Rc::new(RefCell::new(markdown::ListHandler)),
        Rc::new(RefCell::new(markdown::TableHandler::new())),
        Rc::new(RefCell::new(markdown::StyledTextHandler)),
        Rc::new(RefCell::new(markdown::CodeHandler)),
        Rc::new(RefCell::new(markdown::WebpageChromeRemover)),
    ];

    html_to_markdown::convert_html_to_markdown(html.as_bytes(), &mut handlers)
        .unwrap_or_else(|_| html.to_string())
}
