mod config;
mod error;
mod fsutil;
mod history;
mod interactive;
mod llm;
mod provider;
mod setup;

use std::env;
use std::process::{Command, ExitCode};

use error::{Error, Result};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("esh: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Cli {
    Help,
    Version,
    Setup,
    History,
    Clear(String),
    Translate {
        english: String,
        exec: bool,
        yes: bool,
    },
}

fn run() -> Result<ExitCode> {
    let args: Vec<String> = env::args().skip(1).collect();

    match parse_args(&args)? {
        Cli::Help => {
            print_usage();
            Ok(ExitCode::SUCCESS)
        }
        Cli::Version => {
            println!("esh {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Cli::Setup => run_setup(),
        Cli::History => show_history(),
        Cli::Clear(english) => clear(&english),
        Cli::Translate { english, exec, yes } => translate(&english, exec, yes),
    }
}

fn parse_args(args: &[String]) -> Result<Cli> {
    if args.is_empty() {
        return Ok(Cli::Help);
    }

    // Subcommands must come first.
    match args[0].as_str() {
        "setup" => return Ok(Cli::Setup),
        "history" => return Ok(Cli::History),
        "--clear" => {
            if args.len() < 2 {
                return Err(Error::other(format!(
                    "missing English text for --clear\n\n{}",
                    usage()
                )));
            }
            return Ok(Cli::Clear(args[1..].join(" ")));
        }
        _ => {}
    }

    let mut exec = false;
    let mut yes = false;
    let mut english: Vec<String> = Vec::new();
    let mut only_positional = false;

    for arg in args {
        if only_positional {
            english.push(arg.clone());
            continue;
        }

        match arg.as_str() {
            "--" => only_positional = true,
            "-x" | "--exec" => exec = true,
            "-y" | "--yes" => yes = true,
            "-h" | "--help" => return Ok(Cli::Help),
            "-V" | "--version" => return Ok(Cli::Version),
            other if other.starts_with('-') && other != "-" => {
                return Err(Error::other(format!(
                    "unknown option '{other}'\n\n{}",
                    usage()
                )));
            }
            _ => english.push(arg.clone()),
        }
    }

    if english.is_empty() {
        return Err(Error::other(format!("missing English text\n\n{}", usage())));
    }
    if yes && !exec {
        return Err(Error::other("--yes can only be used together with --exec"));
    }

    Ok(Cli::Translate {
        english: english.join(" "),
        exec,
        yes,
    })
}

/// Translates English into a shell command, using the history when possible.
///
/// Without `--exec` only the command is printed, so the output can be used
/// directly (for example inside `$(...)`). With `--exec` the command is run.
fn translate(english: &str, exec: bool, yes: bool) -> Result<ExitCode> {
    let settings = load_settings()?;
    let mut store = history::Store::open(&fsutil::history_path()?)?;

    let command = match store.find(english) {
        Some(command) => command,
        None => {
            let command = llm::translate(&settings, english)?;
            store.add(english, &command)?;
            command
        }
    };

    if exec {
        execute(&command, yes).map(ExitCode::from)
    } else {
        println!("{command}");
        Ok(ExitCode::SUCCESS)
    }
}

/// Shows the command, optionally asks for confirmation, then runs it.
///
/// Returns the exit code of the executed command so it can be propagated.
fn execute(command: &str, assume_yes: bool) -> Result<u8> {
    // Echo to stderr so stdout carries only the executed command's output.
    eprintln!("{command}");

    if !assume_yes && !interactive::confirm("Run this command? [y/N] ", false)? {
        eprintln!("esh: aborted");
        return Ok(0);
    }

    spawn_shell(command)
}

fn spawn_shell(command: &str) -> Result<u8> {
    let status = shell_command(command)
        .status()
        .map_err(|error| Error::other(format!("could not run the command: {error}")))?;

    if let Some(code) = status.code() {
        return Ok(code.clamp(0, 255) as u8);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return Ok((128 + signal).clamp(0, 255) as u8);
        }
    }

    Err(Error::other("the command was terminated"))
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut process = Command::new("cmd");
        process.arg("/C").arg(command);
        process
    }
    #[cfg(not(windows))]
    {
        let mut process = Command::new("sh");
        process.arg("-c").arg(command);
        process
    }
}

fn run_setup() -> Result<ExitCode> {
    let store = config::SettingsStore::from_env()?;
    setup::run(&store)?;
    Ok(ExitCode::SUCCESS)
}

fn show_history() -> Result<ExitCode> {
    let store = history::Store::open(&fsutil::history_path()?)?;

    if store.entries().is_empty() {
        println!("No history yet.");
        return Ok(ExitCode::SUCCESS);
    }

    for (index, entry) in store.entries().iter().enumerate() {
        println!("{:>3}. {}", index + 1, entry.english);
        println!("     {}", entry.command);
    }
    Ok(ExitCode::SUCCESS)
}

fn clear(english: &str) -> Result<ExitCode> {
    let mut store = history::Store::open(&fsutil::history_path()?)?;

    match store.remove(english)? {
        0 => eprintln!("esh: no history entry found for '{english}'"),
        1 => println!("Removed 1 history entry."),
        removed => println!("Removed {removed} history entries."),
    }
    Ok(ExitCode::SUCCESS)
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
     esh <english>              translate English into a shell command\n  \
     esh --exec <english>       translate, then run the command (asks first)\n  \
     esh --exec --yes <english> translate and run without asking\n  \
     esh setup                  choose or change the LLM provider and model\n  \
     esh history                list previously translated commands\n  \
     esh --clear <english>      remove an entry from the history\n  \
     esh --help                 show this help\n  \
     esh --version              show the version\n\
     \n\
     Options:\n  \
     -x, --exec    run the translated command instead of only printing it\n  \
     -y, --yes     skip the confirmation prompt (requires --exec)"
}

fn print_usage() {
    println!("{}", usage());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn translate_cli(english: &str, exec: bool, yes: bool) -> Cli {
        Cli::Translate {
            english: english.to_string(),
            exec,
            yes,
        }
    }

    #[test]
    fn empty_args_show_help() {
        assert_eq!(parse_args(&[]).unwrap(), Cli::Help);
    }

    #[test]
    fn parses_plain_translation() {
        assert_eq!(
            parse_args(&args(&["list files"])).unwrap(),
            translate_cli("list files", false, false)
        );
    }

    #[test]
    fn joins_multiple_words() {
        assert_eq!(
            parse_args(&args(&["list", "all", "files"])).unwrap(),
            translate_cli("list all files", false, false)
        );
    }

    #[test]
    fn parses_exec_and_yes_flags_in_any_order() {
        assert_eq!(
            parse_args(&args(&["-x", "-y", "list files"])).unwrap(),
            translate_cli("list files", true, true)
        );
        assert_eq!(
            parse_args(&args(&["list", "files", "--exec", "--yes"])).unwrap(),
            translate_cli("list files", true, true)
        );
        assert_eq!(
            parse_args(&args(&["--exec", "list files"])).unwrap(),
            translate_cli("list files", true, false)
        );
    }

    #[test]
    fn double_dash_treats_the_rest_as_text() {
        assert_eq!(
            parse_args(&args(&["--", "-x", "is", "part", "of", "the", "text"])).unwrap(),
            translate_cli("-x is part of the text", false, false)
        );
    }

    #[test]
    fn parses_subcommands() {
        assert_eq!(parse_args(&args(&["setup"])).unwrap(), Cli::Setup);
        assert_eq!(parse_args(&args(&["history"])).unwrap(), Cli::History);
        assert_eq!(
            parse_args(&args(&["--clear", "list", "files"])).unwrap(),
            Cli::Clear("list files".to_string())
        );
    }

    #[test]
    fn parses_help_and_version() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Cli::Help);
        assert_eq!(parse_args(&args(&["-h"])).unwrap(), Cli::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Cli::Version);
        assert_eq!(parse_args(&args(&["-V"])).unwrap(), Cli::Version);
    }

    #[test]
    fn rejects_unknown_option() {
        let error = parse_args(&args(&["--bogus"])).unwrap_err();
        assert!(error.to_string().contains("unknown option"), "was: {error}");
    }

    #[test]
    fn rejects_missing_text_and_orphan_yes() {
        assert!(parse_args(&args(&["--exec"])).is_err());
        assert!(parse_args(&args(&["--clear"])).is_err());
        let error = parse_args(&args(&["--yes", "list files"])).unwrap_err();
        assert!(error.to_string().contains("--exec"), "was: {error}");
    }

    #[cfg(unix)]
    #[test]
    fn spawn_shell_propagates_exit_code() {
        assert_eq!(spawn_shell("exit 0").unwrap(), 0);
        assert_eq!(spawn_shell("exit 3").unwrap(), 3);
    }
}
