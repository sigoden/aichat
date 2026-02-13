# Tables Test

## Simple Table (2-3 Columns)

| Name | Age | City |
|------|-----|------|
| Alice | 30 | New York |
| Bob | 25 | Los Angeles |
| Charlie | 35 | Chicago |

## Table with Alignment

| Left Aligned | Center Aligned | Right Aligned |
|:-------------|:--------------:|--------------:|
| Left | Center | Right |
| A | B | C |
| 1 | 2 | 3 |

## Multi-Column Table (5+ Columns)

| ID | Name | Email | Department | Salary | Status |
|----|------|-------|------------|--------|--------|
| 1 | Alice Smith | alice@example.com | Engineering | $120,000 | Active |
| 2 | Bob Johnson | bob@example.com | Marketing | $90,000 | Active |
| 3 | Charlie Brown | charlie@example.com | Sales | $85,000 | Inactive |
| 4 | Diana Prince | diana@example.com | HR | $95,000 | Active |
| 5 | Eve Adams | eve@example.com | Finance | $110,000 | Active |

## Table with Long Text Cells

| Feature | Description | Status |
|---------|-------------|--------|
| Markdown Rendering | Supports full markdown syntax including tables, lists, code blocks, and inline formatting | Implemented |
| Syntax Highlighting | Uses syntect for code highlighting with support for 100+ languages | Implemented |
| Streaming Output | Renders markdown as it arrives from LLM, providing real-time feedback to users | In Progress |
| Performance | Optimized for low latency with event aggregation and efficient buffer management | Planned |

## Table with Code in Cells

| Language | Example | Output |
|----------|---------|--------|
| Rust | `println!("Hello")` | Hello |
| Python | `print("Hello")` | Hello |
| JavaScript | `console.log("Hello")` | Hello |

## Table with Links

| Resource | URL | Description |
|----------|-----|-------------|
| Rust Docs | [https://doc.rust-lang.org](https://doc.rust-lang.org) | Official Rust documentation |
| GitHub | [https://github.com](https://github.com) | Code hosting platform |
| Stack Overflow | [https://stackoverflow.com](https://stackoverflow.com) | Q&A for developers |

## Empty Table

| Column 1 | Column 2 | Column 3 |
|----------|----------|----------|

## Table with Special Characters

| Symbol | Name | Unicode |
|--------|------|---------|
| → | Right Arrow | U+2192 |
| ← | Left Arrow | U+2190 |
| ✓ | Check Mark | U+2713 |
| ✗ | Cross Mark | U+2717 |
| © | Copyright | U+00A9 |

## Narrow Table

| A | B |
|---|---|
| 1 | 2 |
| 3 | 4 |

## Wide Table (Should Test Wrapping)

| Column 1 | Column 2 | Column 3 | Column 4 | Column 5 | Column 6 | Column 7 | Column 8 |
|----------|----------|----------|----------|----------|----------|----------|----------|
| Data 1 | Data 2 | Data 3 | Data 4 | Data 5 | Data 6 | Data 7 | Data 8 |
| Value A | Value B | Value C | Value D | Value E | Value F | Value G | Value H |
