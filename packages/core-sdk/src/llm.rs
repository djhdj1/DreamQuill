use anyhow::{anyhow, Result};
use async_stream::try_stream;
use futures_util::Stream;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};
use std::pin::Pin;

use crate::models::{Message, Provider};

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderKind {
    OpenAI,
    OpenAIResponse,
    Claude,
    Gemini,
}

fn provider_kind(provider: &Provider) -> ProviderKind {
    match provider.provider_type.to_ascii_lowercase().as_str() {
        "claude" | "anthropic" => ProviderKind::Claude,
        "gemini" | "google" => ProviderKind::Gemini,
        "openai-response" => ProviderKind::OpenAIResponse,
        _ => ProviderKind::OpenAI,
    }
}

/**
 * \brief 以统一接口返回流式增量；对于不支持流式的 Provider，会退化为一次性结果。
 */
pub async fn stream_chat<'a>(
    provider: &'a Provider,
    messages: &'a [Message],
) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send + 'a>>> {
    match provider_kind(provider) {
        ProviderKind::OpenAI | ProviderKind::OpenAIResponse => {
            stream_openai(provider, messages).await
        }
        _ => {
            let full = chat_once(provider, messages).await?;
            let s = try_stream! {
                if !full.is_empty() {
                    yield full;
                }
            };
            Ok(Box::pin(s))
        }
    }
}

/**
 * \brief 非流式调用，返回完整回复。
 */
pub async fn chat_once(provider: &Provider, messages: &[Message]) -> Result<String> {
    match provider_kind(provider) {
        ProviderKind::OpenAI | ProviderKind::OpenAIResponse => {
            chat_once_openai(provider, messages).await
        }
        ProviderKind::Claude => chat_once_claude(provider, messages).await,
        ProviderKind::Gemini => chat_once_gemini(provider, messages).await,
    }
}

/**
 * \brief 列出当前 Provider 可用模型列表。
 */
pub async fn list_models(provider: &Provider) -> Result<Vec<String>> {
    match provider_kind(provider) {
        ProviderKind::OpenAI | ProviderKind::OpenAIResponse => list_models_openai(provider).await,
        ProviderKind::Claude => list_models_claude(provider).await,
        ProviderKind::Gemini => list_models_gemini(provider).await,
    }
}

async fn stream_openai<'a>(
    provider: &'a Provider,
    messages: &'a [Message],
) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send + 'a>>> {
    let url = format!(
        "{}/v1/chat/completions",
        provider.api_base.trim_end_matches('/')
    );
    let client = reqwest::Client::builder().build()?;
    let body = json!({
        "model": provider.model,
        "messages": messages,
        "stream": true
    });

    let resp = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", provider.api_key))
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("request failed: {} -> {}", status, text));
    }

    let mut stream = resp.bytes_stream();
    let mut buf = Vec::<u8>::new();

    let out = try_stream! {
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buf.extend_from_slice(&chunk);
            loop {
                if let Some(pos) = find_double_newline(&buf) {
                    let block = buf.drain(..pos + 2).collect::<Vec<u8>>();
                    if let Some(line) = extract_data_line(&block) {
                        if line.trim() == "[DONE]" {
                            break;
                        }
                        if let Some(delta) = parse_openai_delta(&line) {
                            yield delta;
                        }
                    }
                } else {
                    break;
                }
            }
        }
        if !buf.is_empty() {
            if let Some(line) = extract_data_line(&buf) {
                if line.trim() != "[DONE]" {
                    if let Some(delta) = parse_openai_delta(&line) {
                        yield delta;
                    }
                }
            }
        }
    };

    Ok(Box::pin(out))
}

async fn chat_once_openai(provider: &Provider, messages: &[Message]) -> Result<String> {
    let url = format!(
        "{}/v1/chat/completions",
        provider.api_base.trim_end_matches('/')
    );
    let client = reqwest::Client::builder().build()?;
    let body = json!({
        "model": provider.model,
        "messages": messages,
        "stream": false
    });

    let resp = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", provider.api_key))
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("request failed: {} -> {}", status, text));
    }
    let v: Value = resp.json().await?;
    Ok(extract_openai_content(&v))
}

async fn list_models_openai(provider: &Provider) -> Result<Vec<String>> {
    let url = format!("{}/v1/models", provider.api_base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header(AUTHORIZATION, format!("Bearer {}", provider.api_key))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("list models failed: {} -> {}", status, text));
    }
    parse_model_list(resp.json().await?)
}

async fn chat_once_claude(provider: &Provider, messages: &[Message]) -> Result<String> {
    let url = format!("{}/v1/messages", provider.api_base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let (system_prompt, payload_messages) = anthropic_payload(messages);

    let mut body = json!({
        "model": provider.model,
        "max_tokens": 1024,
        "messages": payload_messages,
    });
    if let Some(sys) = system_prompt {
        body["system"] = json!(sys);
    }

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("x-api-key", HeaderValue::from_str(&provider.api_key)?);
    headers.insert(
        "anthropic-version",
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );

    let resp = client.post(url).headers(headers).json(&body).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("claude request failed: {} -> {}", status, text));
    }
    let v: Value = resp.json().await?;
    Ok(extract_anthropic_content(&v))
}

async fn list_models_claude(provider: &Provider) -> Result<Vec<String>> {
    let url = format!("{}/v1/models", provider.api_base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert("x-api-key", HeaderValue::from_str(&provider.api_key)?);
    headers.insert(
        "anthropic-version",
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    let resp = client.get(url).headers(headers).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("claude list models failed: {} -> {}", status, text));
    }
    parse_model_list(resp.json().await?)
}

async fn chat_once_gemini(provider: &Provider, messages: &[Message]) -> Result<String> {
    let base = normalize_gemini_base(&provider.api_base);
    let url = format!("{}/models/{}:generateContent", base, provider.model);
    let client = reqwest::Client::new();
    let (system_prompt, contents) = gemini_payload(messages);

    let mut body = json!({
        "contents": contents,
    });
    if let Some(sys) = system_prompt {
        body["system_instruction"] = json!({
            "parts": [{"text": sys}]
        });
    }

    let resp = client
        .post(url)
        .query(&[("key", provider.api_key.as_str())])
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("gemini request failed: {} -> {}", status, text));
    }
    let v: Value = resp.json().await?;
    Ok(extract_gemini_content(&v))
}

async fn list_models_gemini(provider: &Provider) -> Result<Vec<String>> {
    let base = normalize_gemini_base(&provider.api_base);
    let url = format!("{}/models", base);
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .query(&[("key", provider.api_key.as_str())])
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("gemini list models failed: {} -> {}", status, text));
    }
    parse_gemini_model_list(resp.json().await?)
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

fn extract_data_line(block: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(block);
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with("data:") {
            return Some(line[5..].trim().to_string());
        }
    }
    None
}

fn parse_openai_delta(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    v.get("choices")?
        .get(0)?
        .get("delta")?
        .get("content")?
        .as_str()
        .map(|s| s.to_string())
}

fn extract_openai_content(v: &Value) -> String {
    v.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string()
}

fn extract_anthropic_content(v: &Value) -> String {
    v.get("content")
        .and_then(|arr| arr.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn extract_gemini_content(v: &Value) -> String {
    if let Some(candidates) = v.get("candidates").and_then(|c| c.as_array()) {
        if let Some(first) = candidates.first() {
            if let Some(content) = first.get("content") {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    return parts
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("");
                }
            }
            if let Some(text) = first.get("output").and_then(|t| t.as_str()) {
                return text.to_string();
            }
        }
    }
    v.get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string()
}

fn anthropic_payload(messages: &[Message]) -> (Option<String>, Vec<Value>) {
    let mut system_parts = Vec::new();
    let mut items = Vec::new();
    for msg in messages {
        match msg.role.as_str() {
            "system" => system_parts.push(msg.content.clone()),
            "assistant" => items.push(json!({
                "role": "assistant",
                "content": [{"type": "text", "text": msg.content}]
            })),
            _ => items.push(json!({
                "role": "user",
                "content": [{"type": "text", "text": msg.content}]
            })),
        }
    }
    let system_prompt = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    (system_prompt, items)
}

fn gemini_payload(messages: &[Message]) -> (Option<String>, Vec<Value>) {
    let mut system_parts = Vec::new();
    let mut contents = Vec::new();
    for msg in messages {
        match msg.role.as_str() {
            "system" => system_parts.push(msg.content.clone()),
            "assistant" => contents.push(json!({
                "role": "model",
                "parts": [{"text": msg.content}]
            })),
            _ => contents.push(json!({
                "role": "user",
                "parts": [{"text": msg.content}]
            })),
        }
    }
    let system_prompt = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    (system_prompt, contents)
}

fn parse_model_list(v: Value) -> Result<Vec<String>> {
    if let Some(arr) = v.get("data").and_then(|x| x.as_array()) {
        Ok(arr
            .iter()
            .filter_map(|item| item.get("id").and_then(|s| s.as_str()))
            .map(|s| s.to_string())
            .collect())
    } else if let Some(arr) = v.as_array() {
        Ok(arr
            .iter()
            .filter_map(|item| {
                item.get("id")
                    .and_then(|s| s.as_str())
                    .or_else(|| item.as_str())
            })
            .map(|s| s.to_string())
            .collect())
    } else {
        Err(anyhow!("unexpected models payload: {}", v))
    }
}

fn normalize_gemini_base(api_base: &str) -> String {
    let trimmed = api_base.trim_end_matches('/');
    if trimmed.ends_with("/v1")
        || trimmed.ends_with("/v1beta")
        || trimmed.contains("/v1/")
        || trimmed.contains("/v1beta/")
    {
        trimmed.to_string()
    } else {
        format!("{}/v1beta", trimmed)
    }
}

fn parse_gemini_model_list(v: Value) -> Result<Vec<String>> {
    if let Some(arr) = v.get("models").and_then(|x| x.as_array()) {
        Ok(arr
            .iter()
            .filter_map(|item| {
                item.get("name")
                    .and_then(|s| s.as_str())
                    .or_else(|| item.get("id").and_then(|s| s.as_str()))
            })
            .map(|s| s.to_string())
            .collect())
    } else {
        Err(anyhow!("unexpected gemini models payload: {}", v))
    }
}
