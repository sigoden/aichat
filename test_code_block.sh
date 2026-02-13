#!/bin/bash

# Test code block rendering
cat << 'EOF' | ./target/debug/aichat --no-stream
请渲染这个代码块：

```python
def hello():
    print("Hello")

    return True
```

测试完成。
EOF
