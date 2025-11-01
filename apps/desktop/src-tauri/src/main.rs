#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dreamquill_core_sdk::{db, llm, telemetry};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tauri_plugin_secure_storage::{OptionsRequest, SecureStorageExt};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ProviderRecordDto {
    id: i64,
    name: String,
    provider: String,
    api_base: String,
    api_key: String,
    model: String,
    is_default: bool,
}

#[derive(Debug, Serialize, Clone)]
struct ProviderStateDto {
    providers: Vec<ProviderRecordDto>,
    default_provider_id: Option<i64>,
    telemetry_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct ProviderRequestDto {
    name: String,
    provider: String,
    api_base: String,
    api_key: String,
    model: String,
    #[serde(default)]
    telemetry_enabled: Option<bool>,
    #[serde(default)]
    set_default: Option<bool>,
}

#[derive(Debug, Serialize, Clone)]
struct ChatSummaryDto {
    id: i64,
    title: String,
    provider_id: Option<i64>,
}

#[derive(Debug, Serialize, Clone)]
struct StoredMessageDto {
    id: i64,
    role: String,
    content: String,
}

#[derive(Debug, Serialize, Clone)]
struct ChatMessagesDto {
    chat_id: i64,
    provider_id: Option<i64>,
    messages: Vec<StoredMessageDto>,
}

#[derive(Debug, Serialize)]
struct ChatResultDto {
    chat_id: i64,
    reply: String,
    logs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BranchRequestDto {
    title: Option<String>,
    until_message_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct BranchResultDto {
    chat_id: i64,
    title: String,
}

#[derive(Debug, Deserialize)]
struct HealthPreviewRequestDto {
    name: Option<String>,
    provider: String,
    api_base: String,
    api_key: String,
    model: String,
}

/**
 * \brief 事件负载：带流标识的通用结构。
 */
#[derive(Debug, Serialize, Clone)]
struct StreamEventPayload<T: Serialize> {
    stream_id: String,
    data: T,
}

/** @brief 管理流式任务的取消令牌。 */
#[derive(Default, Clone)]
struct StreamRegistry {
    inner: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl StreamRegistry {
    fn register(&self, stream_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        let mut guard = self.inner.lock().expect("lock stream registry");
        guard.insert(stream_id.to_string(), token.clone());
        token
    }

    fn cancel(&self, stream_id: &str) {
        let mut guard = self.inner.lock().expect("lock stream registry");
        if let Some(token) = guard.remove(stream_id) {
            token.cancel();
        }
    }

    fn remove(&self, stream_id: &str) {
        let mut guard = self.inner.lock().expect("lock stream registry");
        guard.remove(stream_id);
    }
}

fn emit_event<T: Serialize>(app: &tauri::AppHandle, name: &str, payload: &StreamEventPayload<T>) {
    /* brief 兼容 Tauri 2：使用 `emit` 广播事件。 */
    if let Err(e) = app.emit(name, payload) {
        eprintln!("emit {} failed: {}", name, e);
    }
}

fn anyhow_to_string(err: anyhow::Error) -> String {
    err.to_string()
}

const SECRET_PREFIX: &str = "provider";

fn provider_secret_alias(id: i64) -> String {
    format!("{SECRET_PREFIX}:{id}")
}

fn store_provider_secret(app: &tauri::AppHandle, alias: &str, key: &str) -> Result<(), String> {
    let req = OptionsRequest {
        prefixed_key: Some(alias.to_string()),
        data: Some(key.to_string()),
        sync: None,
        keychain_access: None,
    };
    app.secure_storage()
        .set_item(app.clone(), req)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn load_provider_secret(app: &tauri::AppHandle, alias: &str) -> Result<Option<String>, String> {
    let req = OptionsRequest {
        prefixed_key: Some(alias.to_string()),
        data: None,
        sync: None,
        keychain_access: None,
    };
    app.secure_storage()
        .get_item(app.clone(), req)
        .map(|resp| resp.data)
        .map_err(|e| e.to_string())
}

fn hydrate_provider_secret(
    app: &tauri::AppHandle,
    provider: &mut dreamquill_core_sdk::models::Provider,
) -> Result<(), String> {
    if provider.api_key.is_empty() {
        if let Some(alias) = provider.secret_alias.clone() {
            if let Some(secret) = load_provider_secret(app, &alias)? {
                provider.api_key = secret;
            } else {
                return Err("未找到模型服务密钥，请重新配置".to_string());
            }
        }
    }
    Ok(())
}

fn build_state(conn: &rusqlite::Connection) -> Result<ProviderStateDto, anyhow::Error> {
    let providers = db::list_providers(conn)?;
    let default_id = db::get_default_provider_id(conn)?;
    let telemetry_enabled = db::get_telemetry_enabled(conn)?;
    telemetry::set_enabled(telemetry_enabled);
    let items = providers
        .into_iter()
        .map(|p| ProviderRecordDto {
            id: p.id,
            name: p.name,
            provider: p.provider_type,
            api_base: p.api_base,
            api_key: if p.secret_alias.is_some() {
                String::new()
            } else {
                p.api_key
            },
            model: p.model,
            is_default: default_id.map(|d| d == p.id).unwrap_or(false),
        })
        .collect();
    Ok(ProviderStateDto {
        providers: items,
        default_provider_id: default_id,
        telemetry_enabled,
    })
}

fn pick_provider(
    app: Option<&tauri::AppHandle>,
    conn: &rusqlite::Connection,
    chat_id: Option<i64>,
    provider_id: Option<i64>,
) -> Result<dreamquill_core_sdk::models::Provider, String> {
    let mut resolved: Option<dreamquill_core_sdk::models::Provider> = None;

    if let Some(chat_id_value) = chat_id {
        let existing = db::get_provider_for_chat(conn, chat_id_value).map_err(anyhow_to_string)?;
        match (existing, provider_id) {
            (Some(current), Some(pid)) if current.id != pid => {
                let provider = db::get_provider_by_id(conn, pid)
                    .map_err(anyhow_to_string)?
                    .ok_or_else(|| "指定的模型服务不存在".to_string())?;
                db::set_chat_provider(conn, chat_id_value, Some(provider.id))
                    .map_err(anyhow_to_string)?;
                resolved = Some(provider);
            }
            (Some(current), _) => {
                resolved = Some(current);
            }
            (None, Some(pid)) => {
                let provider = db::get_provider_by_id(conn, pid)
                    .map_err(anyhow_to_string)?
                    .ok_or_else(|| "指定的模型服务不存在".to_string())?;
                db::set_chat_provider(conn, chat_id_value, Some(provider.id))
                    .map_err(anyhow_to_string)?;
                resolved = Some(provider);
            }
            (None, None) => {
                let provider = db::get_default_provider(conn)
                    .map_err(anyhow_to_string)?
                    .ok_or_else(|| "尚未配置模型服务，请先创建模型服务".to_string())?;
                db::set_chat_provider(conn, chat_id_value, Some(provider.id))
                    .map_err(anyhow_to_string)?;
                resolved = Some(provider);
            }
        }
    }

    if resolved.is_none() {
        if let Some(pid) = provider_id {
            let provider = db::get_provider_by_id(conn, pid)
                .map_err(anyhow_to_string)?
                .ok_or_else(|| "指定的模型服务不存在".to_string())?;
            resolved = Some(provider);
        } else {
            let provider = db::get_default_provider(conn)
                .map_err(anyhow_to_string)?
                .ok_or_else(|| "尚未配置模型服务，请先创建模型服务".to_string())?;
            resolved = Some(provider);
        }
    }

    let mut provider = resolved.ok_or_else(|| "未找到可用的模型服务".to_string())?;

    if let Some(app_handle) = app {
        if provider.secret_alias.is_none() && !provider.api_key.is_empty() {
            let alias = provider_secret_alias(provider.id);
            store_provider_secret(app_handle, &alias, &provider.api_key)?;
            db::update_provider(
                conn,
                provider.id,
                &provider.name,
                &provider.provider_type,
                &provider.api_base,
                "",
                &provider.model,
                Some(alias.as_str()),
            )
            .map_err(anyhow_to_string)?;
            provider.secret_alias = Some(alias);
        }
        hydrate_provider_secret(app_handle, &mut provider)?;
    }
    Ok(provider)
}

#[tauri::command]
async fn dq_get_config() -> Result<ProviderStateDto, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    build_state(&conn).map_err(anyhow_to_string)
}

#[tauri::command]
async fn dq_create_provider(
    app: tauri::AppHandle,
    payload: ProviderRequestDto,
) -> Result<ProviderStateDto, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    if let Some(enabled) = payload.telemetry_enabled {
        db::set_telemetry_enabled(&conn, enabled).map_err(anyhow_to_string)?;
        telemetry::set_enabled(enabled);
    }
    let key_input_trimmed = payload.api_key.trim();
    let sanitized_api_key = if key_input_trimmed.is_empty() {
        payload.api_key.clone()
    } else {
        String::new()
    };
    let id = if payload.set_default.unwrap_or(false) {
        db::upsert_default_provider(
            &conn,
            &payload.name,
            &payload.provider,
            &payload.api_base,
            &sanitized_api_key,
            &payload.model,
            None,
        )
        .map_err(anyhow_to_string)?
    } else {
        db::insert_provider(
            &conn,
            &payload.name,
            &payload.provider,
            &payload.api_base,
            &sanitized_api_key,
            &payload.model,
            None,
        )
        .map_err(anyhow_to_string)?
    };
    if !key_input_trimmed.is_empty() {
        let alias = provider_secret_alias(id);
        store_provider_secret(&app, &alias, &payload.api_key)?;
        db::set_provider_secret_alias(&conn, id, Some(&alias)).map_err(anyhow_to_string)?;
    } else {
        db::set_provider_secret_alias(&conn, id, None).map_err(anyhow_to_string)?;
    }
    telemetry::log_event(
        "desktop.provider",
        &format!("create name={} type={}", payload.name, payload.provider),
    );
    build_state(&conn).map_err(anyhow_to_string)
}

#[tauri::command]
async fn dq_update_provider(
    app: tauri::AppHandle,
    id: i64,
    payload: ProviderRequestDto,
) -> Result<ProviderStateDto, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    let existing = db::get_provider_by_id(&conn, id)
        .map_err(anyhow_to_string)?
        .ok_or_else(|| "指定的 Provider 不存在".to_string())?;

    let key_input_trimmed = payload.api_key.trim();
    let mut alias = existing.secret_alias.clone();
    let mut db_key = existing.api_key.clone();

    if !key_input_trimmed.is_empty() {
        let alias_value = alias.clone().unwrap_or_else(|| provider_secret_alias(id));
        store_provider_secret(&app, &alias_value, &payload.api_key)?;
        alias = Some(alias_value);
        db_key = String::new();
    } else if alias.is_some() {
        db_key = String::new();
    }

    db::update_provider(
        &conn,
        id,
        &payload.name,
        &payload.provider,
        &payload.api_base,
        &db_key,
        &payload.model,
        alias.as_deref(),
    )
    .map_err(anyhow_to_string)?;
    if payload.set_default.unwrap_or(false) {
        db::set_default_provider_id(&conn, id).map_err(anyhow_to_string)?;
    }
    if let Some(enabled) = payload.telemetry_enabled {
        db::set_telemetry_enabled(&conn, enabled).map_err(anyhow_to_string)?;
        telemetry::set_enabled(enabled);
    }
    telemetry::log_event(
        "desktop.provider",
        &format!("update id={} name={}", id, payload.name),
    );
    build_state(&conn).map_err(anyhow_to_string)
}

#[tauri::command]
async fn dq_delete_provider(app: tauri::AppHandle, id: i64) -> Result<ProviderStateDto, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    if let Some(provider) = db::get_provider_by_id(&conn, id).map_err(anyhow_to_string)? {
        if let Some(alias) = provider.secret_alias {
            let _ = store_provider_secret(&app, &alias, "");
        }
    }
    db::delete_provider(&conn, id).map_err(anyhow_to_string)?;
    telemetry::log_event("desktop.provider", &format!("delete id={}", id));
    build_state(&conn).map_err(anyhow_to_string)
}

#[tauri::command]
async fn dq_select_provider(id: i64) -> Result<ProviderStateDto, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    db::set_default_provider_id(&conn, id).map_err(anyhow_to_string)?;
    telemetry::log_event("desktop.provider", &format!("select-default id={}", id));
    build_state(&conn).map_err(anyhow_to_string)
}

#[tauri::command]
async fn dq_list_chats() -> Result<Vec<ChatSummaryDto>, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    let chats = db::list_chats(&conn, None).map_err(anyhow_to_string)?;
    Ok(chats
        .into_iter()
        .map(|chat| ChatSummaryDto {
            id: chat.id,
            title: chat.title,
            provider_id: chat.provider_id,
        })
        .collect())
}

#[tauri::command]
async fn dq_get_chat_messages(chat_id: i64) -> Result<ChatMessagesDto, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    let provider = db::get_provider_for_chat(&conn, chat_id).map_err(anyhow_to_string)?;
    let messages = db::load_messages_with_meta(&conn, chat_id).map_err(anyhow_to_string)?;
    Ok(ChatMessagesDto {
        chat_id,
        provider_id: provider.map(|p| p.id),
        messages: messages
            .into_iter()
            .map(|msg| StoredMessageDto {
                id: msg.id,
                role: msg.role,
                content: msg.content,
            })
            .collect(),
    })
}

#[tauri::command]
async fn dq_delete_chat(chat_id: i64) -> Result<Vec<ChatSummaryDto>, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    db::delete_chat(&conn, chat_id).map_err(anyhow_to_string)?;
    let chats = db::list_chats(&conn, None).map_err(anyhow_to_string)?;
    Ok(chats
        .into_iter()
        .map(|chat| ChatSummaryDto {
            id: chat.id,
            title: chat.title,
            provider_id: chat.provider_id,
        })
        .collect())
}

#[tauri::command]
async fn dq_branch_chat(
    chat_id: i64,
    payload: BranchRequestDto,
) -> Result<BranchResultDto, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    let telemetry_enabled = db::get_telemetry_enabled(&conn).map_err(anyhow_to_string)?;
    telemetry::set_enabled(telemetry_enabled);

    let title = payload
        .title
        .unwrap_or_else(|| format!("Chat {} 分支", chat_id));
    let new_chat_id = db::clone_chat_until(&conn, chat_id, &title, payload.until_message_id)
        .map_err(anyhow_to_string)?;
    telemetry::log_event(
        "desktop.chat",
        &format!(
            "branch chat={} -> new_chat={} until={:?}",
            chat_id, new_chat_id, payload.until_message_id
        ),
    );
    Ok(BranchResultDto {
        chat_id: new_chat_id,
        title,
    })
}

#[tauri::command]
async fn dq_rename_chat(chat_id: i64, title: String) -> Result<ChatSummaryDto, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("会话标题不能为空".to_string());
    }

    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    db::update_chat_title(&conn, chat_id, trimmed)
        .map_err(anyhow_to_string)?;
    let provider = db::get_provider_for_chat(&conn, chat_id).map_err(anyhow_to_string)?;
    telemetry::log_event(
        "desktop.chat",
        &format!("rename chat id={} title={}", chat_id, trimmed),
    );
    Ok(ChatSummaryDto {
        id: chat_id,
        title: trimmed.to_string(),
        provider_id: provider.map(|p| p.id),
    })
}

#[tauri::command]
async fn dq_list_models(
    app: tauri::AppHandle,
    provider_id: Option<i64>,
) -> Result<Vec<String>, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    let provider = pick_provider(Some(&app), &conn, None, provider_id)?;
    llm::list_models(&provider).await.map_err(anyhow_to_string)
}

#[tauri::command]
async fn dq_send_chat(
    app: tauri::AppHandle,
    prompt: String,
    chat_id: Option<i64>,
    provider_id: Option<i64>,
    stream: Option<bool>,
    debug: Option<bool>,
    regen_message_id: Option<i64>,
) -> Result<ChatResultDto, String> {
    let prompt_trimmed = prompt.trim();
    if regen_message_id.is_some() && !prompt_trimmed.is_empty() {
        return Err("prompt 与 regen_message_id 不可同时提供".to_string());
    }

    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;

    let provider = pick_provider(Some(&app), &conn, chat_id, provider_id)?;
    let telemetry_enabled = db::get_telemetry_enabled(&conn).map_err(anyhow_to_string)?;
    telemetry::set_enabled(telemetry_enabled);

    let chat_id = match chat_id {
        Some(id) => id,
        None => {
            if regen_message_id.is_some() {
                return Err("重新生成需要指定会话 ID".to_string());
            }
            db::create_chat(&conn, &format!("{} 会话", provider.name), provider.id)
                .map_err(anyhow_to_string)?
        }
    };

    if let Some(message_id) = regen_message_id {
        let metas = db::load_messages_with_meta(&conn, chat_id).map_err(anyhow_to_string)?;
        let target = metas
            .iter()
            .find(|msg| msg.id == message_id)
            .ok_or_else(|| "待重新生成的消息不存在".to_string())?;
        if target.role != "assistant" {
            return Err("仅支持对助手消息重新生成".to_string());
        }
        db::delete_messages_from(&conn, chat_id, message_id).map_err(anyhow_to_string)?;
    } else {
        if prompt_trimmed.is_empty() {
            return Err("发送内容不能为空".to_string());
        }
        db::insert_message(&conn, chat_id, "user", prompt_trimmed).map_err(anyhow_to_string)?;
    }

    let messages = db::load_messages(&conn, chat_id).map_err(anyhow_to_string)?;

    let mut logs = Vec::new();
    let debug_flag = debug.unwrap_or(false);
    if debug_flag {
        logs.push(format!(
            "request -> provider={} type={} base={} model={} chat_id={} msgs={}",
            provider.name,
            provider.provider_type,
            provider.api_base,
            provider.model,
            chat_id,
            messages.len()
        ));
    }

    telemetry::log_event(
        "desktop.chat",
        &format!(
            "provider={}({}) chat_id={} action={} prompt_len={}",
            provider.name,
            provider.provider_type,
            chat_id,
            if regen_message_id.is_some() {
                "regenerate"
            } else {
                "send"
            },
            if regen_message_id.is_some() {
                0
            } else {
                prompt_trimmed.len()
            }
        ),
    );

    let prefer_stream = stream.unwrap_or(true);
    let mut reply = String::new();

    if prefer_stream {
        match llm::stream_chat(&provider, &messages).await {
            Ok(mut s) => {
                while let Some(item) = s.as_mut().next().await {
                    match item {
                        Ok(delta) => reply.push_str(&delta),
                        Err(err) => {
                            let msg = format!("stream err: {}", err);
                            logs.push(msg.clone());
                            telemetry::log_error("desktop.chat", &msg);
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                let msg = format!("stream failed: {}", err);
                logs.push(msg.clone());
                telemetry::log_error("desktop.chat", &msg);
                reply = llm::chat_once(&provider, &messages)
                    .await
                    .map_err(anyhow_to_string)?;
            }
        }
    } else {
        reply = llm::chat_once(&provider, &messages)
            .await
            .map_err(anyhow_to_string)?;
    }

    if reply.is_empty() {
        return Err("模型未返回任何内容".to_string());
    }

    db::insert_message(&conn, chat_id, "assistant", &reply).map_err(anyhow_to_string)?;

    Ok(ChatResultDto {
        chat_id,
        reply,
        logs,
    })
}

/**
 * \brief 流式聊天（通过事件推送到前端）。
 * \details 前端需监听 `dq:meta`/`dq:log`/`dq:chunk`/`dq:error`/`dq:end`，并根据 `stream_id` 过滤所属事件。
 */
#[tauri::command]
async fn dq_send_chat_stream(
    app: tauri::AppHandle,
    stream_id: String,
    prompt: String,
    chat_id: Option<i64>,
    provider_id: Option<i64>,
    stream: Option<bool>,
    debug: Option<bool>,
    regen_message_id: Option<i64>,
    registry_state: tauri::State<'_, StreamRegistry>,
) -> Result<(), String> {
    let prompt_trimmed = prompt.trim();
    if regen_message_id.is_some() && !prompt_trimmed.is_empty() {
        return Err("prompt 与 regen_message_id 不可同时提供".to_string());
    }

    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;

    let provider = pick_provider(Some(&app), &conn, chat_id, provider_id)?;
    // 事件通道标识
    let sid = stream_id.clone();

    // 创建/绑定会话
    let chat_id = match chat_id {
        Some(id) => id,
        None => {
            if regen_message_id.is_some() {
                return Err("重新生成需要指定会话 ID".to_string());
            }
            db::create_chat(&conn, &format!("{} 会话", provider.name), provider.id)
                .map_err(anyhow_to_string)?
        }
    };

    if let Some(message_id) = regen_message_id {
        let metas = db::load_messages_with_meta(&conn, chat_id).map_err(anyhow_to_string)?;
        let target = metas
            .iter()
            .find(|msg| msg.id == message_id)
            .ok_or_else(|| "待重新生成的消息不存在".to_string())?;
        if target.role != "assistant" {
            return Err("仅支持对助手消息重新生成".to_string());
        }
        db::delete_messages_from(&conn, chat_id, message_id).map_err(anyhow_to_string)?;
    } else {
        if prompt_trimmed.is_empty() {
            return Err("发送内容不能为空".to_string());
        }
        db::insert_message(&conn, chat_id, "user", prompt_trimmed).map_err(anyhow_to_string)?;
    }

    let messages = db::load_messages(&conn, chat_id).map_err(anyhow_to_string)?;

    // meta 事件
    emit_event(
        &app,
        "dq:meta",
        &StreamEventPayload {
            stream_id: sid.clone(),
            data: serde_json::json!({"chat_id": chat_id}),
        },
    );

    let action_label = if regen_message_id.is_some() {
        "regenerate"
    } else {
        "send"
    };
    let prompt_len = if regen_message_id.is_some() {
        0
    } else {
        prompt_trimmed.len()
    };

    let debug = debug.unwrap_or(false);
    if debug {
        emit_event(
            &app,
            "dq:log",
            &StreamEventPayload {
                stream_id: sid.clone(),
                data: format!(
                    "request -> provider={} type={} base={} model={} chat_id={} msgs={}",
                    provider.name,
                    provider.provider_type,
                    provider.api_base,
                    provider.model,
                    chat_id,
                    messages.len()
                ),
            },
        );
    }

    // 记录遥测
    let telemetry_enabled = db::get_telemetry_enabled(&conn).map_err(anyhow_to_string)?;
    telemetry::set_enabled(telemetry_enabled);
    telemetry::log_event(
        "desktop.chat.stream",
        &format!(
            "provider={}({}) chat_id={} action={} prompt_len={}",
            provider.name, provider.provider_type, chat_id, action_label, prompt_len
        ),
    );

    let prefer_stream = stream.unwrap_or(true);
    let app2 = app.clone();
    let registry = StreamRegistry {
        inner: registry_state.inner.clone(),
    };
    let cancel_token = registry.register(&sid);

    // 后台任务：推送增量并持久化助手回复
    tokio::spawn(async move {
        let mut assistant_buf = String::new();

        if prefer_stream {
            match llm::stream_chat(&provider, &messages).await {
                Ok(s) => {
                    use futures_util::StreamExt;
                    let mut stream = s;
                    loop {
                        tokio::select! {
                            _ = cancel_token.cancelled() => {
                                emit_event(
                                    &app2,
                                    "dq:log",
                                    &StreamEventPayload {
                                        stream_id: sid.clone(),
                                        data: "用户已取消当前回复".to_string(),
                                    },
                                );
                                break;
                            }
                            item = stream.next() => {
                                match item {
                                    Some(Ok(delta)) => {
                                        assistant_buf.push_str(&delta);
                                        emit_event(
                                            &app2,
                                            "dq:chunk",
                                            &StreamEventPayload { stream_id: sid.clone(), data: delta },
                                        );
                                    }
                                    Some(Err(e)) => {
                                        telemetry::log_error(
                                            "desktop.chat.stream",
                                            &format!("stream error: {}", e),
                                        );
                                        emit_event(
                                            &app2,
                                            "dq:error",
                                            &StreamEventPayload {
                                                stream_id: sid.clone(),
                                                data: format!("{}", e),
                                            },
                                        );
                                        break;
                                    }
                                    None => break,
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    telemetry::log_error("desktop.chat.stream", &format!("stream failed: {}", e));
                    // 回退一次性
                    match llm::chat_once(&provider, &messages).await {
                        Ok(full) => {
                            if !cancel_token.is_cancelled() {
                                if !full.is_empty() {
                                    assistant_buf.push_str(&full);
                                    emit_event(
                                        &app2,
                                        "dq:chunk",
                                        &StreamEventPayload {
                                            stream_id: sid.clone(),
                                            data: full,
                                        },
                                    );
                                } else {
                                    emit_event(
                                        &app2,
                                        "dq:error",
                                        &StreamEventPayload {
                                            stream_id: sid.clone(),
                                            data: "模型未返回任何内容".to_string(),
                                        },
                                    );
                                }
                            }
                        }
                        Err(e2) => {
                            emit_event(
                                &app2,
                                "dq:error",
                                &StreamEventPayload {
                                    stream_id: sid.clone(),
                                    data: format!("chat_once failed: {}", e2),
                                },
                            );
                        }
                    }
                }
            }
        } else {
            match llm::chat_once(&provider, &messages).await {
                Ok(full) => {
                    if !cancel_token.is_cancelled() {
                        if !full.is_empty() {
                            assistant_buf.push_str(&full);
                            emit_event(
                                &app2,
                                "dq:chunk",
                                &StreamEventPayload {
                                    stream_id: sid.clone(),
                                    data: full,
                                },
                            );
                        } else {
                            emit_event(
                                &app2,
                                "dq:error",
                                &StreamEventPayload {
                                    stream_id: sid.clone(),
                                    data: "模型未返回任何内容".to_string(),
                                },
                            );
                        }
                    }
                }
                Err(e) => {
                    telemetry::log_error(
                        "desktop.chat.stream",
                        &format!("chat_once failed: {}", e),
                    );
                    emit_event(
                        &app2,
                        "dq:error",
                        &StreamEventPayload {
                            stream_id: sid.clone(),
                            data: format!("{}", e),
                        },
                    );
                }
            }
        }

        // 持久化助手回复
        if !assistant_buf.is_empty() {
            if let Ok(conn2) = db::open_default_db() {
                let _ = db::insert_message(&conn2, chat_id, "assistant", &assistant_buf);
            }
        }

        registry.remove(&sid);

        // 结束事件
        emit_event(
            &app2,
            "dq:end",
            &StreamEventPayload {
                stream_id: sid.clone(),
                data: serde_json::json!({"chat_id": chat_id}),
            },
        );
    });

    Ok(())
}

/** @brief 取消指定流式聊天任务。 */
#[tauri::command]
async fn dq_cancel_stream(
    stream_id: String,
    registry_state: tauri::State<'_, StreamRegistry>,
) -> Result<(), String> {
    let registry = StreamRegistry {
        inner: registry_state.inner.clone(),
    };
    registry.cancel(&stream_id);
    Ok(())
}

/**
 * \brief Provider 健康检查：尝试列出模型，返回可用性。
 */
#[tauri::command]
async fn dq_health_check(
    app: tauri::AppHandle,
    provider_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    let provider = pick_provider(Some(&app), &conn, None, provider_id)?;
    match llm::list_models(&provider).await {
        Ok(list) => Ok(serde_json::json!({
            "ok": true,
            "provider_id": provider.id,
            "provider": provider.provider_type,
            "base": provider.api_base,
            "model": provider.model,
            "models": list.len()
        })),
        Err(e) => Ok(serde_json::json!({
            "ok": false,
            "provider_id": provider.id,
            "provider": provider.provider_type,
            "base": provider.api_base,
            "model": provider.model,
            "error": e.to_string()
        })),
    }
}

#[tauri::command]
async fn dq_health_check_preview(
    app: tauri::AppHandle,
    payload: HealthPreviewRequestDto,
) -> Result<serde_json::Value, String> {
    let conn = db::open_default_db().map_err(anyhow_to_string)?;
    db::migrate(&conn).map_err(anyhow_to_string)?;
    let telemetry_enabled = db::get_telemetry_enabled(&conn).map_err(anyhow_to_string)?;
    telemetry::set_enabled(telemetry_enabled);

    let provider = dreamquill_core_sdk::models::Provider {
        id: -1,
        name: payload
            .name
            .unwrap_or_else(|| "临时健康检查".to_string()),
        provider_type: payload.provider,
        api_base: payload.api_base,
        api_key: payload.api_key,
        model: payload.model,
        secret_alias: None,
    };

    match llm::list_models(&provider).await {
        Ok(list) => Ok(serde_json::json!({
            "ok": true,
            "provider_id": provider.id,
            "provider": provider.provider_type,
            "base": provider.api_base,
            "model": provider.model,
            "models": list.len()
        })),
        Err(e) => Ok(serde_json::json!({
            "ok": false,
            "provider_id": provider.id,
            "provider": provider.provider_type,
            "base": provider.api_base,
            "model": provider.model,
            "error": e.to_string()
        })),
    }
}

fn main() {
    tauri::Builder::default()
        .manage(StreamRegistry::default())
        .plugin(tauri_plugin_secure_storage::init())
        .setup(|_app| {
            if let Ok(conn) = db::open_default_db() {
                let _ = db::migrate(&conn);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dq_get_config,
            dq_create_provider,
            dq_update_provider,
            dq_delete_provider,
            dq_select_provider,
            dq_list_chats,
            dq_get_chat_messages,
            dq_delete_chat,
            dq_branch_chat,
            dq_rename_chat,
            dq_list_models,
            dq_send_chat,
            dq_send_chat_stream,
            dq_cancel_stream,
            dq_health_check,
            dq_health_check_preview
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
