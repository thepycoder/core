use reqwest::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as TokioMutex;

pub const MAX_RETRIES: u32 = 5;
pub const INITIAL_BACKOFF_MS: u64 = 2_000;
pub const MAX_BACKOFF_MS: u64 = 60_000;

pub struct RateLimiter {
    interval_ms: u64,
    min_next_call: TokioMutex<Instant>,
}

impl RateLimiter {
    pub fn new(requests_per_second: f64) -> Self {
        let interval_ms = (1_000.0 / requests_per_second * 1.15) as u64;
        Self {
            interval_ms,
            min_next_call: TokioMutex::new(Instant::now()),
        }
    }

    pub async fn acquire(&self) {
        let mut guard = self.min_next_call.lock().await;
        let now = Instant::now();
        if now < *guard {
            tokio::time::sleep(*guard - now).await;
        }
        *guard = Instant::now() + Duration::from_millis(self.interval_ms);
    }

    pub async fn penalize(&self, delay_ms: u64) {
        let mut guard = self.min_next_call.lock().await;
        *guard = Instant::now() + Duration::from_millis(delay_ms);
    }
}

#[derive(Debug, Clone)]
pub struct WebsearchConversationResult {
    pub text: String,
    pub reference_urls: Vec<String>,
    pub web_search_calls: u32,
}

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    content: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Choice {
    message: Message,
}

#[derive(Serialize, Deserialize, Debug)]
struct ApiResponse {
    choices: Vec<Choice>,
}

#[derive(Serialize, Deserialize, Debug)]
struct AgentResponse {
    id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ConversationResponse {
    outputs: Vec<Value>,
}

pub async fn mistral_complete(
    client: &Client,
    api_key: &str,
    system: &str,
    user: &str,
    model: &str,
    rate_limiter: &RateLimiter,
) -> Option<String> {
    let payload = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user",   "content": user }
        ]
    });

    let json_resp: ApiResponse = post_json_with_retry(
        client,
        api_key,
        "https://api.mistral.ai/v1/chat/completions",
        payload,
        rate_limiter,
        "chat/completions",
    )
    .await?;

    json_resp
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
}

/// Create a Mistral agent with the built-in `web_search` connector.
pub async fn create_websearch_agent(
    client: &Client,
    api_key: &str,
    model: &str,
    name: &str,
    instructions: &str,
    rate_limiter: &RateLimiter,
) -> Option<String> {
    let payload = json!({
        "model": model,
        "name": name,
        "description": "Identifies non-MP parliamentary actors using web search.",
        "instructions": instructions,
        "tools": [{ "type": "web_search" }],
        "completion_args": {
            "temperature": 0.3,
            "top_p": 0.95,
        }
    });

    let resp: AgentResponse = post_json_with_retry(
        client,
        api_key,
        "https://api.mistral.ai/v1/agents",
        payload,
        rate_limiter,
        "agents",
    )
    .await?;

    Some(resp.id)
}

/// Run a one-shot conversation with a web-search-enabled agent (`store: false`).
pub async fn mistral_websearch_conversation(
    client: &Client,
    api_key: &str,
    agent_id: &str,
    inputs: &str,
    rate_limiter: &RateLimiter,
) -> Option<WebsearchConversationResult> {
    let payload = json!({
        "agent_id": agent_id,
        "inputs": inputs,
        "stream": false,
        "store": false,
    });

    let resp: ConversationResponse = post_json_with_retry(
        client,
        api_key,
        "https://api.mistral.ai/v1/conversations",
        payload,
        rate_limiter,
        "conversations",
    )
    .await?;

    Some(parse_conversation_outputs(&resp.outputs))
}

pub fn parse_conversation_outputs(outputs: &[Value]) -> WebsearchConversationResult {
    let mut text = String::new();
    let mut reference_urls = Vec::new();
    let mut web_search_calls = 0u32;

    for output in outputs {
        match output.get("type").and_then(|t| t.as_str()) {
            Some("tool.execution")
                if output.get("name").and_then(|n| n.as_str()) == Some("web_search") =>
            {
                web_search_calls += 1;
            }
            Some("message.output") => {
                let Some(content) = output.get("content") else {
                    continue;
                };
                if let Some(chunks) = content.as_array() {
                    for chunk in chunks {
                        match chunk.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                if let Some(part) = chunk.get("text").and_then(|t| t.as_str()) {
                                    text.push_str(part);
                                }
                            }
                            Some("tool_reference") => {
                                if let Some(url) = chunk.get("url").and_then(|u| u.as_str()) {
                                    if !reference_urls.iter().any(|u| u == url) {
                                        reference_urls.push(url.to_string());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                } else if let Some(part) = content.as_str() {
                    text.push_str(part);
                }
            }
            _ => {}
        }
    }

    WebsearchConversationResult {
        text: text.trim().to_string(),
        reference_urls,
        web_search_calls,
    }
}

async fn post_json_with_retry<T>(
    client: &Client,
    api_key: &str,
    url: &str,
    payload: Value,
    rate_limiter: &RateLimiter,
    api_label: &str,
) -> Option<T>
where
    T: DeserializeOwned,
{
    let mut attempt = 0u32;
    let mut backoff_ms = INITIAL_BACKOFF_MS;

    loop {
        attempt += 1;
        rate_limiter.acquire().await;

        let response = client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, format!("Bearer {api_key}"))
            .json(&payload)
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                return resp.json().await.ok();
            }
            Ok(resp) if resp.status().as_u16() == 429 || resp.status().is_server_error() => {
                let status = resp.status();
                let retry_after_ms = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|secs| secs * 1_000 + 500)
                    .unwrap_or(backoff_ms);
                let body = resp.text().await.unwrap_or_default();
                if attempt >= MAX_RETRIES {
                    eprintln!(
                        "[mistral/{api_label}] retry failed after {attempt} attempts ({status}): {body}"
                    );
                    return None;
                }
                eprintln!(
                    "[mistral/{api_label}] {status} (attempt {attempt}/{MAX_RETRIES}), retrying in {retry_after_ms}ms"
                );
                rate_limiter.penalize(retry_after_ms).await;
                backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                eprintln!("[mistral/{api_label}] request failed ({status}): {body}");
                return None;
            }
            Err(e) => {
                if attempt >= MAX_RETRIES {
                    eprintln!("[mistral/{api_label}] network error after {attempt} attempts: {e}");
                    return None;
                }
                eprintln!("[mistral/{api_label}] network error (attempt {attempt}): {e}");
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
            }
        }
    }
}

pub fn strip_json_fences(raw: &str) -> String {
    let stripped = raw.trim();
    if stripped.starts_with("```") {
        let inner: Vec<&str> = stripped.lines().collect();
        let start = 1;
        let end = inner
            .iter()
            .rposition(|l| l.trim() == "```")
            .unwrap_or(inner.len());
        inner[start..end].join("\n")
    } else {
        stripped.to_string()
    }
}

pub fn hash_text(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_websearch_conversation_outputs() {
        let outputs = vec![
            json!({
                "type": "tool.execution",
                "name": "web_search",
            }),
            json!({
                "type": "message.output",
                "content": [
                    {
                        "type": "text",
                        "text": "{\"identified_as\": \"Jan Jambon\", \"role\": \"minister-president\", "
                    },
                    {
                        "type": "tool_reference",
                        "tool": "web_search",
                        "url": "https://www.vlaanderen.be/jan-jambon",
                    },
                    {
                        "type": "text",
                        "text": "\"confidence\": \"high\"}"
                    }
                ]
            }),
        ];

        let parsed = parse_conversation_outputs(&outputs);
        assert_eq!(parsed.web_search_calls, 1);
        assert_eq!(
            parsed.reference_urls,
            vec!["https://www.vlaanderen.be/jan-jambon"]
        );
        assert!(parsed.text.contains("Jan Jambon"));
    }
}
