//! Provider-agnostic client: lists models and translates English to shell.

use serde::{Deserialize, Serialize};

use crate::config::Settings;
use crate::error::{Error, Result};
use crate::provider::{self, Descriptor, Kind};

/// Fetches the model names available from `descriptor` at `base_url`.
pub fn list_models(
    descriptor: &Descriptor,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<String>> {
    let mut models = match descriptor.kind {
        Kind::Ollama => ollama_models(base_url, api_key)?,
        Kind::OpenAi => openai_models(base_url, api_key)?,
        Kind::Anthropic => anthropic_models(base_url, api_key)?,
        Kind::Gemini => gemini_models(base_url, api_key)?,
    };

    models.sort();
    models.dedup();
    Ok(models)
}

/// Asks the configured provider to turn `english` into a shell command.
pub fn translate(settings: &Settings, english: &str) -> Result<String> {
    let descriptor = provider::lookup(&settings.provider).ok_or_else(|| {
        Error::other(format!(
            "unknown provider '{}'; run `esh setup` to reconfigure",
            settings.provider
        ))
    })?;

    let prompt = build_prompt(english);
    let raw = match descriptor.kind {
        Kind::Ollama => {
            ollama_generate(&settings.server_url, settings.api_key.as_deref(), &settings.model, &prompt)?
        }
        Kind::OpenAi => {
            openai_chat(&settings.server_url, settings.api_key.as_deref(), &settings.model, &prompt)?
        }
        Kind::Anthropic => anthropic_messages(
            &settings.server_url,
            settings.api_key.as_deref(),
            &settings.model,
            &prompt,
        )?,
        Kind::Gemini => {
            gemini_generate(&settings.server_url, settings.api_key.as_deref(), &settings.model, &prompt)?
        }
    };

    let command = sanitize(&raw);
    if command.is_empty() {
        return Err(Error::other("the model did not return a command"));
    }
    Ok(command)
}

/// Builds the instruction sent to the model. Deliberately strict so the raw
/// response can be used as a command with little cleanup.
pub fn build_prompt(english: &str) -> String {
    format!(
        "You translate plain-English requests into a single shell command for the user's shell.\n\
         Rules:\n\
         - Reply with ONLY the shell command.\n\
         - Do not include explanations, comments, or markdown code fences.\n\
         - Prefer widely available POSIX tools unless the request implies otherwise.\n\
         \n\
         Request: {english}\n\
         Command:"
    )
}

/// Strips whitespace and any markdown code fences the model may have added.
pub fn sanitize(raw: &str) -> String {
    let trimmed = raw.trim();

    let content = if let Some(rest) = trimmed.strip_prefix("```") {
        // Drop the info string (e.g. "bash") that may follow the opening fence.
        let rest = match rest.find('\n') {
            Some(index) => &rest[index + 1..],
            None => rest,
        };
        match rest.trim_end().strip_suffix("```") {
            Some(inner) => inner,
            None => rest,
        }
    } else if trimmed.len() >= 2 && trimmed.starts_with('`') && trimmed.ends_with('`') {
        trimmed.trim_matches('`')
    } else {
        trimmed
    };

    content.trim().to_string()
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), path)
}

fn bearer(request: ureq::Request, api_key: Option<&str>) -> ureq::Request {
    match api_key {
        Some(key) if !key.trim().is_empty() => {
            request.set("Authorization", &format!("Bearer {}", key.trim()))
        }
        _ => request,
    }
}

fn required_key<'a>(provider_label: &str, api_key: Option<&'a str>) -> Result<&'a str> {
    api_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| Error::other(format!("{provider_label} requires an API key")))
}

fn http_error(error: ureq::Error) -> Error {
    match error {
        ureq::Error::Status(code, response) => {
            let detail = response.into_string().unwrap_or_default();
            let detail = detail.trim();
            if detail.is_empty() {
                Error::http(format!("the server returned HTTP {code}"))
            } else {
                Error::http(format!("the server returned HTTP {code}: {detail}"))
            }
        }
        ureq::Error::Transport(transport) => {
            Error::http(format!("could not reach the server: {transport}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Ollama (native API)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OllamaTags {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    #[serde(default)]
    response: String,
}

fn ollama_models(base_url: &str, api_key: Option<&str>) -> Result<Vec<String>> {
    let request = bearer(ureq::get(&endpoint(base_url, "/api/tags")), api_key);
    let response: OllamaTags = request.call().map_err(http_error)?.into_json()?;
    Ok(response.models.into_iter().map(|model| model.name).collect())
}

fn ollama_generate(
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    prompt: &str,
) -> Result<String> {
    let body = OllamaRequest {
        model,
        prompt,
        stream: false,
    };
    let request = bearer(ureq::post(&endpoint(base_url, "/api/generate")), api_key);
    let response: OllamaResponse = request.send_json(&body).map_err(http_error)?.into_json()?;
    Ok(response.response)
}

// ---------------------------------------------------------------------------
// OpenAI and OpenAI-compatible services
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OpenAiModels {
    #[serde(default)]
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMessage,
}

#[derive(Deserialize)]
struct OpenAiChoiceMessage {
    #[serde(default)]
    content: String,
}

fn openai_models(base_url: &str, api_key: Option<&str>) -> Result<Vec<String>> {
    let request = bearer(ureq::get(&endpoint(base_url, "/models")), api_key);
    let response: OpenAiModels = request.call().map_err(http_error)?.into_json()?;
    Ok(response.data.into_iter().map(|model| model.id).collect())
}

fn openai_chat(
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    prompt: &str,
) -> Result<String> {
    let body = OpenAiRequest {
        model,
        messages: vec![OpenAiMessage {
            role: "user",
            content: prompt,
        }],
    };
    let request = bearer(ureq::post(&endpoint(base_url, "/chat/completions")), api_key);
    let response: OpenAiResponse = request.send_json(&body).map_err(http_error)?.into_json()?;
    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| Error::other("the model returned no choices"))
}

// ---------------------------------------------------------------------------
// Anthropic (Messages API)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AnthropicModels {
    #[serde(default)]
    data: Vec<AnthropicModel>,
}

#[derive(Deserialize)]
struct AnthropicModel {
    id: String,
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicBlock>,
}

#[derive(Deserialize)]
struct AnthropicBlock {
    #[serde(default)]
    text: Option<String>,
}

fn anthropic_headers(request: ureq::Request, api_key: Option<&str>) -> Result<ureq::Request> {
    let key = required_key("Anthropic", api_key)?;
    Ok(request
        .set("x-api-key", key)
        .set("anthropic-version", "2023-06-01"))
}

fn anthropic_models(base_url: &str, api_key: Option<&str>) -> Result<Vec<String>> {
    let request = anthropic_headers(ureq::get(&endpoint(base_url, "/models")), api_key)?;
    let response: AnthropicModels = request.call().map_err(http_error)?.into_json()?;
    Ok(response.data.into_iter().map(|model| model.id).collect())
}

fn anthropic_messages(
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    prompt: &str,
) -> Result<String> {
    let body = AnthropicRequest {
        model,
        max_tokens: 1024,
        messages: vec![AnthropicMessage {
            role: "user",
            content: prompt,
        }],
    };
    let request = anthropic_headers(ureq::post(&endpoint(base_url, "/messages")), api_key)?;
    let response: AnthropicResponse = request.send_json(&body).map_err(http_error)?.into_json()?;
    Ok(response
        .content
        .into_iter()
        .filter_map(|block| block.text)
        .collect::<Vec<_>>()
        .join(""))
}

// ---------------------------------------------------------------------------
// Google Gemini
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GeminiModels {
    #[serde(default)]
    models: Vec<GeminiModel>,
}

#[derive(Deserialize)]
struct GeminiModel {
    name: String,
}

#[derive(Serialize)]
struct GeminiRequest<'a> {
    contents: Vec<GeminiContentIn<'a>>,
}

#[derive(Serialize)]
struct GeminiContentIn<'a> {
    parts: Vec<GeminiPartIn<'a>>,
}

#[derive(Serialize)]
struct GeminiPartIn<'a> {
    text: &'a str,
}

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContentOut>,
}

#[derive(Deserialize)]
struct GeminiContentOut {
    #[serde(default)]
    parts: Vec<GeminiPartOut>,
}

#[derive(Deserialize)]
struct GeminiPartOut {
    #[serde(default)]
    text: Option<String>,
}

fn gemini_headers(request: ureq::Request, api_key: Option<&str>) -> Result<ureq::Request> {
    let key = required_key("Google Gemini", api_key)?;
    Ok(request.set("x-goog-api-key", key))
}

fn gemini_models(base_url: &str, api_key: Option<&str>) -> Result<Vec<String>> {
    let request = gemini_headers(ureq::get(&endpoint(base_url, "/models")), api_key)?;
    let response: GeminiModels = request.call().map_err(http_error)?.into_json()?;
    Ok(response
        .models
        .into_iter()
        .map(|model| model.name.trim_start_matches("models/").to_string())
        .collect())
}

fn gemini_generate(
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    prompt: &str,
) -> Result<String> {
    let model = model.trim_start_matches("models/");
    let path = format!("/models/{model}:generateContent");
    let body = GeminiRequest {
        contents: vec![GeminiContentIn {
            parts: vec![GeminiPartIn { text: prompt }],
        }],
    };
    let request = gemini_headers(ureq::post(&endpoint(base_url, &path)), api_key)?;
    let response: GeminiResponse = request.send_json(&body).map_err(http_error)?.into_json()?;
    Ok(response
        .candidates
        .into_iter()
        .filter_map(|candidate| candidate.content)
        .flat_map(|content| content.parts)
        .filter_map(|part| part.text)
        .collect::<Vec<_>>()
        .join(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;

    fn serve(body: &'static str) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        serve_status("200 OK", body)
    }

    fn serve_status(
        status: &'static str,
        body: &'static str,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            let _ = tx.send(request);

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });

        (format!("http://{addr}"), rx, handle)
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut data = Vec::new();
        let mut buffer = [0u8; 1024];
        let mut expected_len = None;

        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    data.extend_from_slice(&buffer[..read]);
                    if expected_len.is_none()
                        && let Some(header_end) = find(&data, b"\r\n\r\n")
                    {
                        let headers = String::from_utf8_lossy(&data[..header_end]).to_lowercase();
                        let content_length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        expected_len = Some(header_end + 4 + content_length);
                    }
                    if let Some(len) = expected_len
                        && data.len() >= len
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        String::from_utf8_lossy(&data).to_string()
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn settings(provider: &str, server_url: String, model: &str, api_key: Option<&str>) -> Settings {
        Settings {
            provider: provider.to_string(),
            server_url,
            model: model.to_string(),
            api_key: api_key.map(str::to_string),
        }
    }

    #[test]
    fn ollama_lists_and_deduplicates_models() {
        let (server, rx, handle) = serve(
            r#"{"models":[{"name":"llama3:latest"},{"name":"codellama"},{"name":"codellama"}]}"#,
        );
        let descriptor = provider::lookup("ollama").unwrap();

        let models = list_models(descriptor, &server, None).unwrap();

        assert_eq!(models, vec!["codellama".to_string(), "llama3:latest".to_string()]);
        handle.join().unwrap();
        assert!(rx.recv().unwrap().starts_with("GET /api/tags"));
    }

    #[test]
    fn ollama_translates_and_sanitizes() {
        let (server, _, handle) = serve(r#"{"response":"```bash\nls -la\n```"}"#);
        let settings = settings("ollama", server, "llama3", None);

        assert_eq!(translate(&settings, "list files").unwrap(), "ls -la");
        handle.join().unwrap();
    }

    #[test]
    fn ollama_sends_api_key_as_bearer_token() {
        let (server, rx, handle) = serve(r#"{"models":[]}"#);
        let descriptor = provider::lookup("ollama").unwrap();

        list_models(descriptor, &server, Some("secret-key")).unwrap();

        handle.join().unwrap();
        let request = rx.recv().unwrap().to_lowercase();
        assert!(
            request.contains("authorization: bearer secret-key"),
            "request was: {request}"
        );
    }

    #[test]
    fn openai_translates_via_chat_completions() {
        let (server, rx, handle) =
            serve(r#"{"choices":[{"message":{"role":"assistant","content":"```\nls -la\n```"}}]}"#);
        let settings = settings("openai", server, "gpt-4o-mini", Some("sk-test"));

        assert_eq!(translate(&settings, "list files").unwrap(), "ls -la");

        handle.join().unwrap();
        let request = rx.recv().unwrap();
        assert!(request.starts_with("POST /chat/completions"), "was: {request}");
        assert!(request.to_lowercase().contains("authorization: bearer sk-test"));
        assert!(request.contains("\"model\":\"gpt-4o-mini\""));
    }

    #[test]
    fn openai_compatible_preset_uses_its_base_url() {
        let (server, rx, handle) = serve(r#"{"data":[{"id":"mixtral-8x7b"},{"id":"llama-3.1-70b"}]}"#);
        let descriptor = provider::lookup("groq").unwrap();

        let models = list_models(descriptor, &server, Some("gsk-test")).unwrap();

        assert_eq!(
            models,
            vec!["llama-3.1-70b".to_string(), "mixtral-8x7b".to_string()]
        );
        handle.join().unwrap();
        assert!(rx.recv().unwrap().starts_with("GET /models"));
    }

    #[test]
    fn anthropic_sends_expected_headers_and_parses_blocks() {
        let (server, rx, handle) =
            serve(r#"{"content":[{"type":"text","text":"ls -la"},{"type":"text","text":" "}]}"#);
        let settings = settings("anthropic", server, "claude-3-5-sonnet-latest", Some("test-key"));

        assert_eq!(translate(&settings, "list files").unwrap(), "ls -la");

        handle.join().unwrap();
        let request = rx.recv().unwrap();
        assert!(request.starts_with("POST /messages"), "was: {request}");
        let lowered = request.to_lowercase();
        assert!(lowered.contains("x-api-key: test-key"));
        assert!(lowered.contains("anthropic-version: 2023-06-01"));
    }

    #[test]
    fn anthropic_requires_an_api_key() {
        let error = anthropic_messages("http://localhost:1", None, "m", "p").unwrap_err();
        assert!(error.to_string().contains("API key"), "was: {error}");
    }

    #[test]
    fn gemini_strips_model_prefix_and_targets_generate_content() {
        let (server, rx, handle) =
            serve(r#"{"candidates":[{"content":{"parts":[{"text":"ls -la"}]}}]}"#);
        let settings = settings("gemini", server, "gemini-2.0-flash", Some("gkey"));

        assert_eq!(translate(&settings, "list files").unwrap(), "ls -la");

        handle.join().unwrap();
        let request = rx.recv().unwrap();
        assert!(
            request.starts_with("POST /models/gemini-2.0-flash:generateContent"),
            "was: {request}"
        );
        assert!(request.to_lowercase().contains("x-goog-api-key: gkey"));
    }

    #[test]
    fn gemini_lists_models_without_prefix() {
        let (server, _, handle) = serve(
            r#"{"models":[{"name":"models/gemini-2.0-flash"},{"name":"models/gemini-1.5-pro"}]}"#,
        );
        let descriptor = provider::lookup("gemini").unwrap();

        let models = list_models(descriptor, &server, Some("gkey")).unwrap();

        assert_eq!(
            models,
            vec!["gemini-1.5-pro".to_string(), "gemini-2.0-flash".to_string()]
        );
        handle.join().unwrap();
    }

    #[test]
    fn errors_on_empty_model_response() {
        let (server, _, handle) = serve(r#"{"response":"   "}"#);
        let settings = settings("ollama", server, "llama3", None);

        let error = translate(&settings, "list files").unwrap_err();

        assert!(error.to_string().contains("did not return a command"));
        handle.join().unwrap();
    }

    #[test]
    fn surfaces_http_error_status() {
        let (server, _, handle) = serve_status("500 Internal Server Error", "boom");
        let descriptor = provider::lookup("ollama").unwrap();

        let error = list_models(descriptor, &server, None).unwrap_err();

        assert!(error.to_string().contains("500"), "was: {error}");
        handle.join().unwrap();
    }

    #[test]
    fn unknown_provider_is_reported() {
        let settings = settings("nope", "http://localhost:1".to_string(), "m", None);
        let error = translate(&settings, "list files").unwrap_err();
        assert!(error.to_string().contains("unknown provider"), "was: {error}");
    }

    #[test]
    fn sanitize_handles_plain_and_fenced_output() {
        assert_eq!(sanitize("  ls -la  "), "ls -la");
        assert_eq!(sanitize("```\nls -la\n```"), "ls -la");
        assert_eq!(sanitize("```sh\nls -la\n```"), "ls -la");
        assert_eq!(sanitize("`ls -la`"), "ls -la");
    }

    #[test]
    fn prompt_includes_the_request() {
        assert!(build_prompt("list files").contains("list files"));
    }
}
