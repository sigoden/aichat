use aichat::render::streamdown_adapter::StreamdownRenderer;
use aichat::render::RenderOptions;

fn main() {
    let options = RenderOptions::default();
    let mut renderer = StreamdownRenderer::new(options).unwrap();

    let markdown = r#"```python
def fib_generator(n):
    a, b = 0, 1
    for _ in range(n):
        yield a
        a, b = b, a + b

# 测试：打印前10个数
print(list(fib_generator(10)))
```"#;

    let output = renderer.render(markdown).unwrap();
    println!("{}", output);

    // 检查空行
    let lines: Vec<&str> = output.lines().collect();
    println!("\n=== 行数: {} ===", lines.len());
    for (i, line) in lines.iter().enumerate() {
        let visible_len = streamdown_ansi::utils::visible_length(line);
        println!("Line {}: visible_len={}, bytes={}", i, visible_len, line.len());
    }
}
