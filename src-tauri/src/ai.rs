//! In-bar AI answers. Calls go straight from here to the provider, local or keyed, and stream
//! back to the bar; keys live in the OS keychain, never in state.json.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const KEYCHAIN_SERVICE: &str = "com.zephyr.app.ai";

const SYSTEM_PROMPT: &str = "You answer questions typed into Zephyr, a launcher bar. Answer \
directly and briefly: lead with the answer, then only the detail that helps. Use plain text; use \
a short list or a code block only when the content needs it.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Api {
    #[default]
    Openai,
    Anthropic,
}

/// Which provider answers, and with which model. An empty model means the provider's default,
/// or the first model a local server reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    pub provider: String,
    pub model: String,
    /// Only used by the custom provider.
    pub base_url: String,
    /// Only used by the custom provider.
    pub api: Api,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: "auto".into(),
            model: String::new(),
            base_url: String::new(),
            api: Api::Openai,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub api: Api,
    pub base_url: &'static str,
    pub needs_key: bool,
    pub local: bool,
    pub default_model: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "ollama",
        name: "Ollama",
        api: Api::Openai,
        base_url: "http://127.0.0.1:11434/v1",
        needs_key: false,
        local: true,
        default_model: "",
    },
    Preset {
        id: "lmstudio",
        name: "LM Studio",
        api: Api::Openai,
        base_url: "http://127.0.0.1:1234/v1",
        needs_key: false,
        local: true,
        default_model: "",
    },
    Preset {
        id: "anthropic",
        name: "Anthropic",
        api: Api::Anthropic,
        base_url: "https://api.anthropic.com/v1",
        needs_key: true,
        local: false,
        default_model: "claude-opus-5-5",
    },
    Preset {
        id: "openai",
        name: "OpenAI",
        api: Api::Openai,
        base_url: "https://api.openai.com/v1",
        needs_key: true,
        local: false,
        default_model: "",
    },
    Preset {
        id: "openrouter",
        name: "OpenRouter",
        api: Api::Openai,
        base_url: "https://openrouter.ai/api/v1",
        needs_key: true,
        local: false,
        default_model: "",
    },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

impl AiSettings {
    pub fn normalized(mut self) -> Result<Self, String> {
        self.provider = self.provider.trim().to_string();
        self.model = self.model.trim().to_string();
        self.base_url = self.base_url.trim().trim_end_matches('/').to_string();
        match self.provider.as_str() {
            "auto" => {}
            "custom" => {
                if !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://"))
                {
                    return Err("The base URL must start with http:// or https://".into());
                }
            }
            id if preset(id).is_some() => {}
            _ => return Err("Choose an AI provider from the list".into()),
        }
        Ok(self)
    }
}

/// Everything one request needs, with the key already read from the keychain.
#[derive(Debug, Clone)]
struct Resolved {
    api: Api,
    base_url: String,
    model: String,
    key: Option<String>,
    provider_name: String,
}

// Keychain

pub fn set_key(provider: &str, key: Option<&str>) -> Result<(), String> {
    if provider != "custom" && !preset(provider).is_some_and(|preset| preset.needs_key) {
        return Err("That provider doesn't use a key".into());
    }
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, provider).map_err(keychain_error)?;
    match key.map(str::trim).filter(|key| !key.is_empty()) {
        Some(key) => entry.set_password(key).map_err(keychain_error),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(keychain_error(err)),
        },
    }
}

pub fn has_key(provider: &str) -> bool {
    read_key(provider).ok().flatten().is_some()
}

fn read_key(provider: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, provider).map_err(keychain_error)?;
    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(keychain_error(err)),
    }
}

fn keychain_error(err: keyring::Error) -> String {
    format!("Couldn't use the keychain: {err}")
}

// HTTP

fn http() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .read_timeout(Duration::from_secs(90))
                .build()
                .map_err(|err| err.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn with_auth(
    request: reqwest::RequestBuilder,
    api: Api,
    key: Option<&str>,
) -> reqwest::RequestBuilder {
    match (api, key) {
        (Api::Anthropic, Some(key)) => request
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        (Api::Anthropic, None) => request.header("anthropic-version", "2023-06-01"),
        (Api::Openai, Some(key)) => request.bearer_auth(key),
        (Api::Openai, None) => request,
    }
}

/// Lists the models a provider offers, newest naming as the provider returns it.
async fn list_models(api: Api, base_url: &str, key: Option<&str>) -> Result<Vec<String>, String> {
    let request = with_auth(http()?.get(format!("{base_url}/models")), api, key)
        .timeout(Duration::from_secs(8));
    let response = request.send().await.map_err(|err| err.to_string())?;
    let status = response.status();
    let body = read_json(response).await?;
    if !status.is_success() {
        return Err(error_message(&body).unwrap_or_else(|| format!("HTTP {status}")));
    }
    let mut models: Vec<String> = body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect();
    if api == Api::Openai && models.is_empty() {
        // Ollama's native listing, for older servers without /v1/models.
        models = body["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|model| model["name"].as_str().map(str::to_string))
            .collect();
    }
    Ok(models)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalServer {
    pub provider: &'static str,
    pub name: &'static str,
    pub models: Vec<String>,
}

/// Local model servers that answer right now, with the models they have loaded.
pub async fn detect_local() -> Vec<LocalServer> {
    let mut found = Vec::new();
    for preset in PRESETS.iter().filter(|preset| preset.local) {
        if let Ok(models) = list_models(preset.api, preset.base_url, None).await
            && !models.is_empty()
        {
            found.push(LocalServer {
                provider: preset.id,
                name: preset.name,
                models,
            });
        }
    }
    found
}

pub async fn models_for(settings: &AiSettings) -> Result<Vec<String>, String> {
    let settings = settings.clone().normalized()?;
    let (api, base_url, key) = match settings.provider.as_str() {
        "auto" => {
            return Ok(detect_local()
                .await
                .into_iter()
                .flat_map(|server| server.models)
                .collect());
        }
        "custom" => (settings.api, settings.base_url.clone(), read_key("custom")?),
        id => {
            let preset = preset(id).ok_or("Unknown provider")?;
            let key = if preset.needs_key {
                Some(read_key(id)?.ok_or(format!("Add your {} key first", preset.name))?)
            } else {
                None
            };
            (preset.api, preset.base_url.to_string(), key)
        }
    };
    list_models(api, &base_url, key.as_deref())
        .await
        .map_err(|err| format!("Couldn't list models: {err}"))
}

async fn resolve(settings: &AiSettings) -> Result<Resolved, String> {
    let settings = settings.clone().normalized()?;
    match settings.provider.as_str() {
        "auto" => {
            let Some(server) = detect_local().await.into_iter().next() else {
                return Err("No AI is set up. Start Ollama or LM Studio, or add an API key in Settings > AI.".into());
            };
            let preset = preset(server.provider).ok_or("Unknown provider")?;
            Ok(Resolved {
                api: preset.api,
                base_url: preset.base_url.to_string(),
                model: server.models[0].clone(),
                key: None,
                provider_name: preset.name.to_string(),
            })
        }
        "custom" => {
            if settings.model.is_empty() {
                return Err("Pick a model in Settings > AI".into());
            }
            Ok(Resolved {
                api: settings.api,
                base_url: settings.base_url,
                model: settings.model,
                key: read_key("custom")?,
                provider_name: "Your server".into(),
            })
        }
        id => {
            let preset = preset(id).ok_or("Unknown provider")?;
            let key = if preset.needs_key {
                Some(
                    read_key(id)?
                        .ok_or(format!("Add your {} key in Settings > AI", preset.name))?,
                )
            } else {
                None
            };
            let model = if !settings.model.is_empty() {
                settings.model
            } else if !preset.default_model.is_empty() {
                preset.default_model.to_string()
            } else if preset.local {
                list_models(preset.api, preset.base_url, None)
                    .await
                    .map_err(|_| format!("{} isn't running", preset.name))?
                    .into_iter()
                    .next()
                    .ok_or(format!("{} has no models loaded", preset.name))?
            } else {
                return Err("Pick a model in Settings > AI".into());
            };
            Ok(Resolved {
                api: preset.api,
                base_url: preset.base_url.to_string(),
                model,
                key,
                provider_name: preset.name.to_string(),
            })
        }
    }
}

// Streaming

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum AiEvent {
    Started { provider: String, model: String },
    Delta { text: String },
    Done,
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Done,
    Error(String),
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Stops whatever answer is streaming; its remaining text is dropped.
pub fn cancel() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Streams an answer to `question` through `emit`. Returns when the answer ends, fails, or a
/// newer ask (or `cancel`) replaces it.
pub async fn ask(settings: &AiSettings, question: &str, emit: impl Fn(AiEvent)) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let current = || GENERATION.load(Ordering::SeqCst) == generation;
    let fail = |message: String| {
        if current() {
            emit(AiEvent::Error { message });
        }
    };

    let resolved = match resolve(settings).await {
        Ok(resolved) => resolved,
        Err(message) => return fail(message),
    };
    if !current() {
        return;
    }
    emit(AiEvent::Started {
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
    });

    let request = match build_request(&resolved, question) {
        Ok(request) => request,
        Err(message) => return fail(message),
    };
    let mut response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            return fail(if err.is_connect() {
                format!("Couldn't reach {}", resolved.provider_name)
            } else {
                err.to_string()
            });
        }
    };
    if !response.status().is_success() {
        let status = response.status();
        let body = read_json(response).await.unwrap_or(Value::Null);
        let detail = error_message(&body).unwrap_or_else(|| format!("HTTP {status}"));
        return fail(format!("{}: {detail}", resolved.provider_name));
    }

    let mut buffer: Vec<u8> = Vec::new();
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(err) => return fail(err.to_string()),
        };
        if !current() {
            return;
        }
        buffer.extend_from_slice(&chunk);
        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let Some(piece) = parse_line(resolved.api, line.trim_end()) else {
                continue;
            };
            match piece {
                Piece::Text(text) => emit(AiEvent::Delta { text }),
                Piece::Done => {
                    emit(AiEvent::Done);
                    return;
                }
                Piece::Error(message) => return fail(message),
            }
        }
    }
    if current() {
        emit(AiEvent::Done);
    }
}

fn build_request(resolved: &Resolved, question: &str) -> Result<reqwest::RequestBuilder, String> {
    let client = http()?;
    let request = match resolved.api {
        Api::Openai => client
            .post(format!("{}/chat/completions", resolved.base_url))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(
                json!({
                    "model": resolved.model,
                    "stream": true,
                    "messages": [
                        {"role": "system", "content": SYSTEM_PROMPT},
                        {"role": "user", "content": question},
                    ],
                })
                .to_string(),
            ),
        Api::Anthropic => {
            let mut body = json!({
                "model": resolved.model,
                "max_tokens": 16000,
                "stream": true,
                "system": SYSTEM_PROMPT,
                "messages": [{"role": "user", "content": question}],
            });
            let mut request = client.post(format!("{}/messages", resolved.base_url));
            if supports_effort(&resolved.model) {
                // A launcher answer should be quick; low effort keeps latency down.
                body["output_config"] = json!({"effort": "low"});
            }
            if supports_default_fallback(&resolved.model) {
                // A safety decline is retried on Anthropic's recommended fallback model.
                body["fallbacks"] = json!("default");
                request = request.header("anthropic-beta", "server-side-fallback-2026-07-01");
            }
            request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_string())
        }
    };
    Ok(with_auth(request, resolved.api, resolved.key.as_deref()))
}

fn supports_effort(model: &str) -> bool {
    [
        "claude-opus-5",
        "claude-sonnet-5",
        "claude-fable-5",
        "claude-opus-4-8",
        "claude-opus-4-7",
    ]
    .iter()
    .any(|prefix| model.starts_with(prefix))
}

fn supports_default_fallback(model: &str) -> bool {
    model.starts_with("claude-opus-5")
        || model == "claude-fable-5-1"
        || model == "claude-sonnet-5-5"
}

/// Reads one server-sent-events line. Event names are ignored: both APIs repeat the type in
/// the data payload.
fn parse_line(api: Api, line: &str) -> Option<Piece> {
    let data = line.strip_prefix("data:")?.trim_start();
    if data == "[DONE]" {
        return Some(Piece::Done);
    }
    let value: Value = serde_json::from_str(data).ok()?;
    if let Some(message) = error_message(&value) {
        return Some(Piece::Error(message));
    }
    match api {
        Api::Openai => {
            let text = value["choices"][0]["delta"]["content"].as_str()?;
            (!text.is_empty()).then(|| Piece::Text(text.to_string()))
        }
        Api::Anthropic => match value["type"].as_str()? {
            "content_block_delta" if value["delta"]["type"] == "text_delta" => {
                Some(Piece::Text(value["delta"]["text"].as_str()?.to_string()))
            }
            "message_delta" if value["delta"]["stop_reason"] == "refusal" => {
                Some(Piece::Error("The model declined to answer that.".into()))
            }
            "message_stop" => Some(Piece::Done),
            _ => None,
        },
    }
}

async fn read_json(response: reqwest::Response) -> Result<Value, String> {
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    serde_json::from_slice(&bytes).map_err(|err| err.to_string())
}

fn error_message(value: &Value) -> Option<String> {
    let error = value.get("error")?;
    error["message"]
        .as_str()
        .or_else(|| error.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_openai_style_deltas() {
        let line = r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#;
        assert_eq!(
            parse_line(Api::Openai, line),
            Some(Piece::Text("Hel".into()))
        );
        assert_eq!(parse_line(Api::Openai, "data: [DONE]"), Some(Piece::Done));
        assert_eq!(
            parse_line(
                Api::Openai,
                r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#
            ),
            None
        );
        assert_eq!(parse_line(Api::Openai, ": keep-alive"), None);
    }

    #[test]
    fn reads_anthropic_deltas_and_stop() {
        let delta = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#;
        assert_eq!(
            parse_line(Api::Anthropic, delta),
            Some(Piece::Text("Hi".into()))
        );
        assert_eq!(parse_line(Api::Anthropic, "event: message_stop"), None);
        assert_eq!(
            parse_line(Api::Anthropic, r#"data: {"type":"message_stop"}"#),
            Some(Piece::Done)
        );
        let refusal = r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal"}}"#;
        assert!(matches!(
            parse_line(Api::Anthropic, refusal),
            Some(Piece::Error(_))
        ));
    }

    #[test]
    fn surfaces_errors_in_the_stream() {
        let line =
            r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        assert_eq!(
            parse_line(Api::Anthropic, line),
            Some(Piece::Error("Overloaded".into()))
        );
    }

    #[test]
    fn rejects_unknown_providers_and_bad_urls() {
        let mut settings = AiSettings {
            provider: "nope".into(),
            ..AiSettings::default()
        };
        assert!(settings.clone().normalized().is_err());
        settings.provider = "custom".into();
        settings.base_url = "localhost:8080".into();
        assert!(settings.clone().normalized().is_err());
        settings.base_url = "http://localhost:8080/v1/".into();
        assert_eq!(
            settings.normalized().unwrap().base_url,
            "http://localhost:8080/v1"
        );
    }
}
