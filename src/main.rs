mod config;
mod error;
mod fsutil;
mod history;
mod ollama;
mod setup;

use std::env;
use std::process::ExitCode;

use error::{Error, Result};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("esh: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None => {
            print_usage();
            Ok(())
        }
        Some("-h") | Some("--help") => {
            print_usage();
            Ok(())
        }
        Some("-V") | Some("--version") => {
            println!("esh {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("history") => show_history(),
        Some("--clear") => clear(&args[1..]),
        Some(flag) if flag.starts_with('-') => {
            Err(Error::other(format!("unknown option '{flag}'\n\n{}", usage())))
        }
        Some(_) => translate(&args),
    }
}

/// Translates English into a shell command, printing only the command so the
/// output can be used directly (for example inside `$(...)`).
fn translate(words: &[String]) -> Result<()> {
    let english = words.join(" ");
    let settings = load_settings()?;
    let mut store = history::Store::open(&fsutil::history_path()?)?;

    if let Some(command) = store.find(&english) {
        println!("{command}");
        return Ok(());
    }

    let command = ollama::translate(&settings, &english)?;
    store.add(&english, &command)?;
    println!("{command}");
    Ok(())
}

fn show_history() -> Result<()> {
    let store = history::Store::open(&fsutil::history_path()?)?;

    if store.entries().is_empty() {
        println!("No history yet.");
        return Ok(());
    }

    for (index, entry) in store.entries().iter().enumerate() {
        println!("{:>3}. {}", index + 1, entry.english);
        println!("     {}", entry.command);
    }
    Ok(())
}

fn clear(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::other(format!(
            "missing English text for --clear\n\n{}",
            usage()
        )));
    }

    let english = args.join(" ");
    let mut store = history::Store::open(&fsutil::history_path()?)?;

    match store.remove(&english)? {
        0 => eprintln!("esh: no history entry found for '{english}'"),
        1 => println!("Removed 1 history entry."),
        removed => println!("Removed {removed} history entries."),
    }
    Ok(())
}

fn load_settings() -> Result<config::Settings> {
    let store = config::SettingsStore::from_env()?;
    match store.load()? {
        Some(settings) => Ok(settings),
        None => setup::run(&store),
    }
}

fn usage() -> &'static str {
    "Usage:\n  \
     esh <english>          translate English into a shell command\n  \
     esh history            list previously translated commands\n  \
     esh --clear <english>  remove an entry from the history\n  \
     esh --help             show this help\n  \
     esh --version          show the version"
}

fn print_usage() {
    println!("{}", usage());
}
