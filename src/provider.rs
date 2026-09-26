//! The set of LLM providers `esh` can talk to.

/// The wire protocol used to talk to a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Ollama's native API (`/api/tags`, `/api/generate`).
    Ollama,
    /// The OpenAI chat-completions API, shared by many compatible services.
    OpenAi,
    /// Anthropic's Messages API.
    Anthropic,
    /// Google's Gemini API.
    Gemini,
}

/// Static description of a supported provider.
pub struct Descriptor {
    /// Stable identifier stored in the configuration.
    pub id: &'static str,
    /// Human-readable name shown in the setup menu.
    pub label: &'static str,
    pub kind: Kind,
    /// Default base URL, or `None` when the user must supply one.
    pub base_url: Option<&'static str>,
    /// Whether an API key is mandatory.
    pub requires_key: bool,
    /// Short hint shown alongside the label.
    pub note: &'static str,
}

/// Provider used when nothing else is configured.
pub const DEFAULT_ID: &str = "ollama";

/// Every provider `esh` knows about, in menu order.
pub const PROVIDERS: &[Descriptor] = &[
    Descriptor {
        id: "ollama",
        label: "Ollama",
        kind: Kind::Ollama,
        base_url: Some("http://localhost:11434"),
        requires_key: false,
        note: "local or self-hosted, no API key needed",
    },
    Descriptor {
        id: "openai",
        label: "OpenAI",
        kind: Kind::OpenAi,
        base_url: Some("https://api.openai.com/v1"),
        requires_key: true,
        note: "",
    },
    Descriptor {
        id: "anthropic",
        label: "Anthropic (Claude)",
        kind: Kind::Anthropic,
        base_url: Some("https://api.anthropic.com/v1"),
        requires_key: true,
        note: "",
    },
    Descriptor {
        id: "gemini",
        label: "Google Gemini",
        kind: Kind::Gemini,
        base_url: Some("https://generativelanguage.googleapis.com/v1beta"),
        requires_key: true,
        note: "",
    },
    Descriptor {
        id: "groq",
        label: "Groq",
        kind: Kind::OpenAi,
        base_url: Some("https://api.groq.com/openai/v1"),
        requires_key: true,
        note: "",
    },
    Descriptor {
        id: "mistral",
        label: "Mistral",
        kind: Kind::OpenAi,
        base_url: Some("https://api.mistral.ai/v1"),
        requires_key: true,
        note: "",
    },
    Descriptor {
        id: "deepseek",
        label: "DeepSeek",
        kind: Kind::OpenAi,
        base_url: Some("https://api.deepseek.com/v1"),
        requires_key: true,
        note: "",
    },
    Descriptor {
        id: "xai",
        label: "xAI (Grok)",
        kind: Kind::OpenAi,
        base_url: Some("https://api.x.ai/v1"),
        requires_key: true,
        note: "",
    },
    Descriptor {
        id: "openrouter",
        label: "OpenRouter",
        kind: Kind::OpenAi,
        base_url: Some("https://openrouter.ai/api/v1"),
        requires_key: true,
        note: "routes to many models",
    },
    Descriptor {
        id: "together",
        label: "Together AI",
        kind: Kind::OpenAi,
        base_url: Some("https://api.together.xyz/v1"),
        requires_key: true,
        note: "",
    },
    Descriptor {
        id: "custom",
        label: "Custom (OpenAI-compatible)",
        kind: Kind::OpenAi,
        base_url: None,
        requires_key: false,
        note: "any server exposing the OpenAI API",
    },
];

/// Looks up a provider by its identifier.
pub fn lookup(id: &str) -> Option<&'static Descriptor> {
    PROVIDERS.iter().find(|descriptor| descriptor.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn default_provider_exists() {
        assert!(lookup(DEFAULT_ID).is_some());
    }

    #[test]
    fn ids_are_unique() {
        let ids: HashSet<_> = PROVIDERS.iter().map(|descriptor| descriptor.id).collect();
        assert_eq!(ids.len(), PROVIDERS.len());
    }

    #[test]
    fn only_custom_lacks_a_default_url() {
        for descriptor in PROVIDERS {
            if descriptor.base_url.is_none() {
                assert_eq!(descriptor.id, "custom");
            }
        }
    }

    #[test]
    fn lookup_rejects_unknown_ids() {
        assert!(lookup("nope").is_none());
    }
}
