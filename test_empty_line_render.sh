#!/bin/bash

# 测试代码块空行渲染
cat << 'EOF' > /tmp/test_empty_lines.md
```python
def fib_generator(n):
    a, b = 0, 1
    for _ in range(n):
        yield a
        a, b = b, a + b

# 测试：打印前10个数
print(list(fib_generator(10)))
```
EOF

echo "=== 测试 markdown 文件 ==="
cat /tmp/test_empty_lines.md

echo ""
echo "=== 使用 aichat 渲染 ==="
./target/release/aichat --no-stream < /tmp/test_empty_lines.md 2>&1 | head -20
