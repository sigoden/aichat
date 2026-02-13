# Complex Markdown Test

This file tests combinations of different markdown elements to simulate real LLM responses.

## Example 1: Table + Code Block

Here's a comparison of different programming languages:

| Language | Paradigm | Type System | Example |
|----------|----------|-------------|---------|
| Rust | Multi-paradigm | Static, Strong | `let x: i32 = 42;` |
| Python | Multi-paradigm | Dynamic, Strong | `x = 42` |
| JavaScript | Multi-paradigm | Dynamic, Weak | `let x = 42;` |

Here's a Rust example:

```rust
fn main() {
    let languages = vec!["Rust", "Python", "JavaScript"];
    for lang in languages {
        println!("Language: {}", lang);
    }
}
```

## Example 2: List + Code Block

Steps to set up a Rust project:

1. Install Rust using rustup
2. Create a new project:

```bash
cargo new my_project
cd my_project
```

3. Add dependencies to `Cargo.toml`:

```toml
[dependencies]
serde = "1.0"
tokio = { version = "1.0", features = ["full"] }
```

4. Build and run:

```bash
cargo build
cargo run
```

## Example 3: Blockquote + List

> **Important Note:**
>
> When working with async Rust, remember:
>
> - Always use `.await` on async functions
> - Use `tokio::spawn` for concurrent tasks
> - Handle errors with `Result<T, E>`
> - Consider using `Arc<Mutex<T>>` for shared state

## Example 4: Complete LLM Response

**Question**: How do I implement a simple HTTP server in Rust?

**Answer**:

Here's a step-by-step guide to implementing an HTTP server in Rust using the `tokio` and `hyper` crates:

### 1. Add Dependencies

First, add these to your `Cargo.toml`:

```toml
[dependencies]
tokio = { version = "1.0", features = ["full"] }
hyper = { version = "0.14", features = ["full"] }
```

### 2. Implementation

```rust
use hyper::{Body, Request, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use std::convert::Infallible;

async fn handle_request(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new(Body::from("Hello, World!")))
}

#[tokio::main]
async fn main() {
    let addr = ([127, 0, 0, 1], 3000).into();

    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, Infallible>(service_fn(handle_request))
    });

    let server = Server::bind(&addr).serve(make_svc);

    println!("Server running on http://{}", addr);

    if let Err(e) = server.await {
        eprintln!("Server error: {}", e);
    }
}
```

### 3. Key Points

| Component | Purpose | Notes |
|-----------|---------|-------|
| `tokio` | Async runtime | Provides the executor for async code |
| `hyper` | HTTP library | Low-level HTTP implementation |
| `make_service_fn` | Service factory | Creates a new service for each connection |
| `service_fn` | Request handler | Converts a function into a service |

### 4. Running the Server

```bash
cargo run
```

Then visit `http://localhost:3000` in your browser.

### 5. Next Steps

- Add routing with `hyper-router` or `warp`
- Implement error handling
- Add middleware for logging
- Support different HTTP methods (GET, POST, etc.)

## Example 5: Nested Structures

### Project Structure

```
my-server/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── handlers/
│   │   ├── mod.rs
│   │   ├── health.rs
│   │   └── api.rs
│   └── models/
│       ├── mod.rs
│       └── user.rs
└── tests/
    └── integration_test.rs
```

### File Contents

**src/handlers/health.rs**:

```rust
use hyper::{Body, Response};

pub async fn health_check() -> Response<Body> {
    Response::new(Body::from("OK"))
}
```

**src/models/user.rs**:

```rust
#[derive(Debug, Clone)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
}

impl User {
    pub fn new(id: u64, name: String, email: String) -> Self {
        Self { id, name, email }
    }
}
```

## Example 6: Mixed Formatting

This example shows **bold**, *italic*, `code`, and [links](https://example.com) all together.

- **Bold list item** with *italic* and `code`
- Item with a [link to Rust docs](https://doc.rust-lang.org)
- Item with inline code: `let x = vec![1, 2, 3];`

| Feature | Status | Code |
|---------|--------|------|
| **Bold** | ✓ | `bold()` |
| *Italic* | ✓ | `italic()` |
| `Code` | ✓ | `code()` |

```rust
// All features combined
fn format_text(text: &str) -> String {
    format!("**{}**", text) // Bold
}
```
