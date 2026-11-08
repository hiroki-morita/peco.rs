//! Run with `cargo run --example arithmetic -- '10 - 2 * (3 + 1)'`.
#[path = "support/arithmetic.rs"]
mod arithmetic;

// Parses a supplied expression and prints its checked integer value.
fn main() -> Result<(), String> {
    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "10 - 2 * (3 + 1)".into());
    let expression = arithmetic::expression()
        .parse(&input)
        .map_err(|error| error.display_with_input(&input).to_string())?;
    let value = expression
        .evaluate()
        .ok_or("integer overflow during evaluation")?;
    println!("{value}");
    Ok(())
}
