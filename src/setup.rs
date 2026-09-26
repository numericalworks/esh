use crate::config::{Settings, SettingsStore};
use crate::error::Result;
use crate::interactive;
use crate::llm;
use crate::provider::{self, Descriptor};

/// Interactive configuration wizard.
///
/// Used on first launch and by `esh setup`, which is how you switch providers,
/// change the server URL or API key, or pick a different model.
pub fn run(store: &SettingsStore) -> Result<Settings> {
    println!("Welcome to esh! Let's set things up.\n");

    loop {
        let descriptor = choose_provider()?;
        let server_url = prompt_server_url(descriptor)?;
        let api_key = prompt_api_key(descriptor)?;

        let models = match llm::list_models(descriptor, &server_url, api_key.as_deref()) {
            Ok(models) => models,
            Err(error) => {
                eprintln!("\nCould not list models: {error}");
                Vec::new()
            }
        };

        let model = if models.is_empty() {
            let input =
                interactive::prompt("Enter a model name (or press Enter to try again): ")?;
            let input = input.trim();
            if input.is_empty() {
                println!();
                continue;
            }
            input.to_string()
        } else {
            choose_model(&models)?
        };

        let settings = Settings {
            provider: descriptor.id.to_string(),
            server_url,
            model,
            api_key,
        };
        store.save(&settings)?;
        println!(
            "\nSettings saved: provider '{}', model '{}'.",
            descriptor.id, settings.model
        );
        return Ok(settings);
    }
}

fn choose_provider() -> Result<&'static Descriptor> {
    println!("Choose your LLM provider:");
    for (index, descriptor) in provider::PROVIDERS.iter().enumerate() {
        let note = if descriptor.note.is_empty() {
            String::new()
        } else {
            format!("  ({})", descriptor.note)
        };
        println!("  {}. {}{note}", index + 1, descriptor.label);
    }

    loop {
        let input = interactive::prompt("Provider [1]: ")?;
        let input = input.trim();

        if input.is_empty() {
            return Ok(&provider::PROVIDERS[0]);
        }

        if let Ok(number) = input.parse::<usize>() {
            if let Some(descriptor) = number
                .checked_sub(1)
                .and_then(|index| provider::PROVIDERS.get(index))
            {
                return Ok(descriptor);
            }
            eprintln!(
                "Please enter a number between 1 and {}.",
                provider::PROVIDERS.len()
            );
            continue;
        }

        if let Some(descriptor) = provider::lookup(input) {
            return Ok(descriptor);
        }

        eprintln!("Unknown provider '{input}'. Choose a number from the list.");
    }
}

fn prompt_server_url(descriptor: &Descriptor) -> Result<String> {
    match descriptor.base_url {
        Some(default) => {
            let input = interactive::prompt(&format!("Server URL [{default}]: "))?;
            let input = input.trim();
            if input.is_empty() {
                Ok(default.to_string())
            } else {
                Ok(input.to_string())
            }
        }
        None => loop {
            let input = interactive::prompt("Server URL: ")?;
            let input = input.trim();
            if input.is_empty() {
                eprintln!("A server URL is required for this provider.");
                continue;
            }
            return Ok(input.to_string());
        },
    }
}

fn prompt_api_key(descriptor: &Descriptor) -> Result<Option<String>> {
    let label = if descriptor.requires_key {
        "API key: "
    } else {
        "API key (optional) [None]: "
    };

    loop {
        let input = rpassword::prompt_password(label)?;
        let input = input.trim();

        if input.is_empty() {
            if descriptor.requires_key {
                eprintln!("This provider requires an API key.");
                continue;
            }
            return Ok(None);
        }
        return Ok(Some(input.to_string()));
    }
}

fn choose_model(models: &[String]) -> Result<String> {
    println!("\nAvailable models:");
    for (index, model) in models.iter().enumerate() {
        println!("  {}. {model}", index + 1);
    }

    loop {
        let input = interactive::prompt("Select a model (number or name) [1]: ")?;
        let input = input.trim();

        if input.is_empty() {
            return Ok(models[0].clone());
        }

        if let Ok(number) = input.parse::<usize>() {
            if let Some(model) = number
                .checked_sub(1)
                .and_then(|index| models.get(index))
            {
                return Ok(model.clone());
            }
            eprintln!("Please enter a number between 1 and {}.", models.len());
            continue;
        }

        // Anything else is treated as a literal model name.
        return Ok(input.to_string());
    }
}
