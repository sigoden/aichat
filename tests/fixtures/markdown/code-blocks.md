# Code Blocks Test

## Rust Code Block

```rust
fn main() {
    println!("Hello, world!");

    let numbers = vec![1, 2, 3, 4, 5];
    let sum: i32 = numbers.iter().sum();

    println!("Sum: {}", sum);
}

struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn distance(&self, other: &Point) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}
```

## Python Code Block

```python
def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)

class Calculator:
    def __init__(self):
        self.result = 0

    def add(self, x, y):
        self.result = x + y
        return self.result

    def multiply(self, x, y):
        self.result = x * y
        return self.result

# Usage
calc = Calculator()
print(calc.add(5, 3))
print(calc.multiply(4, 7))
```

## JavaScript Code Block

```javascript
const fetchData = async (url) => {
    try {
        const response = await fetch(url);
        const data = await response.json();
        return data;
    } catch (error) {
        console.error('Error fetching data:', error);
        throw error;
    }
};

class TodoList {
    constructor() {
        this.todos = [];
    }

    addTodo(text) {
        this.todos.push({ text, completed: false });
    }

    completeTodo(index) {
        if (this.todos[index]) {
            this.todos[index].completed = true;
        }
    }
}
```

## Code Block Without Language

```
This is a code block without a language identifier.
It should still be rendered with a monospace font.

Line 1
Line 2
Line 3
```

## Long Code Block (Exceeds Terminal Width)

```rust
fn very_long_function_name_that_exceeds_typical_terminal_width(parameter1: String, parameter2: i32, parameter3: Vec<String>) -> Result<String, Box<dyn std::error::Error>> {
    let very_long_variable_name_that_also_exceeds_terminal_width = format!("Processing {} with {} items", parameter1, parameter3.len());
    println!("This is a very long line that will definitely exceed the typical 80-character terminal width and should test wrapping behavior");
    Ok(very_long_variable_name_that_also_exceeds_terminal_width)
}
```

## Multiple Code Blocks in Sequence

```rust
let x = 42;
```

```python
x = 42
```

```javascript
const x = 42;
```

## Code Block with Special Characters

```bash
echo "Hello, $USER!"
grep -r "pattern" /path/to/dir | awk '{print $1}'
sed 's/old/new/g' file.txt > output.txt
```
