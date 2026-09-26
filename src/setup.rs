use crate::config::{Settings, SettingsStore};
use crate::error::{Error, Result};
use crate::interactive;
use crate::ollama;

const DEFAULT_SERVER_URL: &str = "http://localhost:11434";

/// Interactive first-launch wizard. Prompts for the server URL and API key,
/// lets the user pick one of the available models, and persists the result.
pub fn run(store: &SettingsStore) -> Result<Settings> {
    println!("Welcome to esh! Let's set things up.\n");

    loop {
        let server_url = prompt_server_url()?;
        let api_key = prompt_api_key()?;

        match ollama::list_models(&server_url, api_key.as_deref()) {
            Ok(models) if !models.is_empty() => {
                let model = choose_model(&models)?;
                let settings = Settings {
                    server_url,
                    model,
                    api_key,
                };
                store.save(&settings)?;
                println!("\nSettings saved. You're ready to go.");
                return Ok(settings);
            }
            Ok(_) => eprintln!("\nThe server reported no available models."),
            Err(error) => eprintln!("\nCould not fetch models: {error}"),
        }

        if !interactive::confirm("Try again? [Y/n] ", true)? {
            return Err(Error::other(
                "setup cancelled: no settings were saved",
            ));
        }
        println!();
    }
}

fn prompt_server_url() -> Result<String> {
    let input = interactive::prompt(&format!("Ollama server URL [{DEFAULT_SERVER_URL}]: "))?;
    let input = input.trim();
    if input.is_empty() {
        Ok(DEFAULT_SERVER_URL.to_string())
    } else {
        Ok(input.to_string())
    }
}

fn prompt_api_key() -> Result<Option<String>> {
    let input = rpassword::prompt_password("Ollama API key [None]: ")?;
    let input = input.trim();
    if input.is_empty() {
        Ok(None)
    } else {
        Ok(Some(input.to_string()))
    }
}

fn choose_model(models: &[String]) -> Result<String> {
    println!("\nAvailable models:");
    for (index, model) in models.iter().enumerate() {
        println!("  {}. {model}", index + 1);
    }

    loop {
        let input = interactive::prompt("Select a model [1]: ")?;
        let input = input.trim();

        if input.is_empty() {
            return Ok(models[0].clone());
        }

        if let Some(model) = input
            .parse::<usize>()
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| models.get(index))
        {
            return Ok(model.clone());
        }

        eprintln!("Please enter a number between 1 and {}.", models.len());
    }
}
