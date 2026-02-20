fn main() {
    // Test Rust highlighting
    let test_code = r#"fn main() {
    let x = 42;
    println!("Hello, world!");
}
"#;

    println!("=== Testing Rust highlighting ===");
    let mut highlighter = torvax::syntax::Highlighter::new();
    let success = highlighter.set_language_from_path("test.rs");
    println!("Language set: {}", success);

    let highlights = highlighter.highlight(test_code);
    println!("Number of highlights: {}", highlights.len());

    for (i, span) in highlights.iter().enumerate().take(10) {
        let text = &test_code[span.start..span.end];
        println!(
            "{}: [{}-{}] {:?} = '{}'",
            i, span.start, span.end, span.token_type, text
        );
    }

    // Test Markdown highlighting
    let markdown_code = r#"# Hello World

This is a **bold** text and *italic* text.

```rust
fn main() {
    println!("Hello");
}
```

- List item 1
- List item 2

[Link](https://example.com)
"#;

    println!("\n=== Testing Markdown highlighting ===");
    let mut md_highlighter = torvax::syntax::Highlighter::new();
    let md_success = md_highlighter.set_language_from_path("test.md");
    println!("Language set: {}", md_success);

    let md_highlights = md_highlighter.highlight(markdown_code);
    println!("Number of highlights: {}", md_highlights.len());

    for (i, span) in md_highlights.iter().enumerate().take(20) {
        let text = &markdown_code[span.start..span.end];
        println!(
            "{}: [{}-{}] {:?} = '{}'",
            i, span.start, span.end, span.token_type, text
        );
    }
}
