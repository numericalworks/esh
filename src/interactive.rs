use std::io::{self, Write};

use crate::error::Result;

/// Prompts on stderr and reads one line from stdin.
///
/// Prompts go to stderr so that stdout stays reserved for command output,
/// which keeps `esh` usable inside pipelines and command substitution.
pub fn prompt(message: &str) -> Result<String> {
    eprint!("{message}");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

/// Asks a yes/no question. An empty answer returns `default_yes`; otherwise
/// only "y" or "yes" (case-insensitive) count as confirmation.
pub fn confirm(message: &str, default_yes: bool) -> Result<bool> {
    let input = prompt(message)?;
    let input = input.trim();
    if input.is_empty() {
        return Ok(default_yes);
    }
    Ok(input.eq_ignore_ascii_case("y") || input.eq_ignore_ascii_case("yes"))
}
