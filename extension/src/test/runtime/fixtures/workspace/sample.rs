// Fixture Rust file for @vscode/test-electron runtime tests.
// Two TODO(@user) markers in line comments. The extension should
// detect both as DocumentLinks and produce a Hover at either.
//
// This fixture intentionally contains non-ASCII content (em-dash —
// and emoji 🎉) *before* the second marker to exercise the UTF-16 /
// UTF-8 offset translation in extension/src/textOffset.ts. If that
// translation regresses, the hover test for TODO(@bob) will fail.

// TODO(@alice) review this fixture before publish — note the dash above
fn main() {
    println!("hello");
}

// 🎉 Non-ASCII shift before TODO(@bob); offset translation must hold.
// TODO(@bob) update the regex if the field set changes
