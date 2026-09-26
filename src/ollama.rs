use serde::{Deserialize, Serialize};

use crate::config::Settings;
use crate::error::{Error, Result};

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct ModelInfo {
    name: String,
}

#[derive(Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

/// Fetches the names of the models the server currently has available.
pub fn list_models(server_url: &str, api_key: Option<&str>) -> Result<Vec<String>> {
    let url = endpoint(server_url, "/api/tags");
    let request = with_auth(ureq::get(&url), api_key);
    let response: TagsResponse = request.call().map_err(http_error)?.into_json()?;

    let mut names: Vec<String> = response.models.into_iter().map(|model| model.name).collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Asks the model to turn `english` into a shell command.
pub fn translate(settings: &Settings, english: &str) -> Result<String> {
    let url = endpoint(&settings.server_url, "/api/generate");
    let body = GenerateRequest {
        model: settings.model.clone(),
        prompt: build_prompt(english),
        stream: false,
    };

    let request = with_auth(ureq::post(&url), settings.api_key.as_deref());
    let response: GenerateResponse = request
        .send_json(&body)
        .map_err(http_error)?
        .into_json()?;

    let command = sanitize(&response.response);
    if command.is_empty() {
        return Err(Error::other("the model did not return a command"));
    }
    Ok(command)
}

/// Builds the instruction sent to the model. The prompt is deliberately
/// strict so the raw response can be used as a command with little cleanup.
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

fn endpoint(server_url: &str, path: &str) -> String {
    format!("{}{}", server_url.trim_end_matches('/'), path)
}

fn with_auth(request: ureq::Request, api_key: Option<&str>) -> ureq::Request {
    match api_key {
        Some(key) if !key.trim().is_empty() => {
            request.set("Authorization", &format!("Bearer {}", key.trim()))
        }
        _ => request,
    }
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
            Error::http(format!("could not reach the Ollama server: {transport}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;

    /// Spawns a one-shot HTTP server that answers with `body` and reports the
    /// received request back through a channel.
    fn serve_once(body: &'static str) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        serve_once_status("200 OK", body)
    }

    fn serve_once_status(
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
        haystack.windows(needle.len()).position(|window| window == needle)
    }

    #[test]
    fn lists_models_sorted_and_deduplicated() {
        let (server, rx, handle) = serve_once(
            r#"{"models":[{"name":"llama3:latest"},{"name":"codellama"},{"name":"codellama"}]}"#,
        );

        let models = list_models(&server, None).unwrap();

        assert_eq!(models, vec!["codellama".to_string(), "llama3:latest".to_string()]);
        handle.join().unwrap();
        assert!(rx.recv().unwrap().starts_with("GET /api/tags"));
    }

    #[test]
    fn sends_api_key_as_bearer_token() {
        let (server, rx, handle) = serve_once(r#"{"models":[]}"#);

        list_models(&server, Some("secret-key")).unwrap();

        handle.join().unwrap();
        let request = rx.recv().unwrap().to_lowercase();
        assert!(request.contains("authorization: bearer secret-key"), "request was: {request}");
    }

    #[test]
    fn sends_no_authorization_header_without_api_key() {
        let (server, rx, handle) = serve_once(r#"{"models":[]}"#);

        list_models(&server, None).unwrap();

        handle.join().unwrap();
        assert!(!rx.recv().unwrap().to_lowercase().contains("authorization:"));
    }

    #[test]
    fn translates_and_sanitizes_response() {
        let (server, _, handle) = serve_once(r#"{"response":"```bash\nls -la\n```"}"#);
        let settings = Settings {
            server_url: server,
            model: "test-model".to_string(),
            api_key: None,
        };

        let command = translate(&settings, "list all files with sizes").unwrap();

        assert_eq!(command, "ls -la");
        handle.join().unwrap();
    }

    #[test]
    fn errors_on_empty_model_response() {
        let (server, _, handle) = serve_once(r#"{"response":"   "}"#);
        let settings = Settings {
            server_url: server,
            model: "test-model".to_string(),
            api_key: None,
        };

        let error = translate(&settings, "list files").unwrap_err();

        assert!(error.to_string().contains("did not return a command"));
        handle.join().unwrap();
    }

    #[test]
    fn surfaces_http_error_status() {
        let (server, _, handle) = serve_once_status("500 Internal Server Error", "boom");

        let error = list_models(&server, None).unwrap_err();

        assert!(error.to_string().contains("500"), "was: {error}");
        handle.join().unwrap();
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
