use std::convert::Infallible;

use anyhow::{anyhow, Result};
use axum::{
    extract::{Path, Query},
    response::sse::{Event, KeepAlive, Sse},
    routing::{delete, get, get_service, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tower_http::services::ServeDir;

use crate::{db, llm, telemetry, models::Provider};

/**
 * \brief 启动本地 HTTP 服务，提供静态前端与 API。
 * \param addr 监听地址，如 "127.0.0.1:5173"
 */
pub async fn run(addr: &str) -> Result<()> {
    let ui_root =
        std::env::var("DREAMQUILL_UI_DIR").unwrap_or_else(|_| "packages/ui/dist".to_string());
    let fallback_root =
        std::env::var("DREAMQUILL_UI_FALLBACK").unwrap_or_else(|_| "web".to_string());

    let static_handler = if std::path::Path::new(&ui_root).exists() {
        ServeDir::new(ui_root)
    } else {
        ServeDir::new(fallback_root)
    }
    .append_index_html_on_directories(true);

    let static_service = get_service(static_handler);

    let app = Router::new()
        .route("/api/config", get(get_config).post(set_config))
        .route("/api/providers", get(get_providers).post(create_provider))
        .route(
            "/api/providers/{id}",
            put(update_provider).delete(delete_provider),
        )
        .route("/api/providers/{id}/select", post(select_provider))
        .route("/api/chats", get(list_chats))
        .route("/api/chats/{id}/messages", get(get_chat_messages))
        .route("/api/chats/{id}", delete(remove_chat).put(rename_chat))
        .route("/api/chats/{id}/branch", post(branch_chat))
        .route("/api/models", get(list_models))
        .route("/api/health", get(health_check))
        .route("/api/health/preview", post(health_check_preview))
        .route("/api/chat/sse", get(chat_sse))
        .fallback_service(static_service);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Server listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
struct ProviderInput {
    /** \brief Provider 名称 */
    #[serde(default)]
    name: Option<String>,
    /** \brief Provider 类型 */
    provider: String,
    /** \brief API 基地址 */
    api_base: String,
    /** \brief API 密钥 */
    api_key: String,
    /** \brief 模型名 */
    model: String,
    #[serde(default)]
    telemetry_enabled: Option<bool>,
    #[serde(default)]
    set_default: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ProviderRequest {
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

#[derive(Serialize, Debug)]
struct ProviderItem {
    id: i64,
    name: String,
    provider: String,
    api_base: String,
    api_key: String,
    model: String,
    is_default: bool,
}

#[derive(Serialize, Debug)]
struct ProvidersState {
    providers: Vec<ProviderItem>,
    default_provider_id: Option<i64>,
    telemetry_enabled: bool,
}

#[derive(Deserialize, Debug)]
struct ModelQuery {
    provider_id: Option<i64>,
}

#[derive(Deserialize, Debug)]
struct ChatListQuery {
    provider_id: Option<i64>,
}

#[derive(Serialize, Debug)]
struct ChatSummaryDto {
    id: i64,
    title: String,
    provider_id: Option<i64>,
}

#[derive(Serialize, Debug)]
struct ChatListResponse {
    chats: Vec<ChatSummaryDto>,
}

#[derive(Serialize, Debug)]
struct ChatMessageDto {
    id: i64,
    role: String,
    content: String,
}

#[derive(Serialize, Debug)]
struct ChatMessagesResponse {
    chat_id: i64,
    provider_id: Option<i64>,
    messages: Vec<ChatMessageDto>,
}

#[derive(Deserialize, Debug)]
struct BranchRequest {
    /** \brief 新聊天标题，可选。 */
    title: Option<String>,
    /** \brief 截断到的消息 ID（包含该消息）。 */
    until_message_id: Option<i64>,
}

#[derive(Serialize, Debug)]
struct BranchResponse {
    chat_id: i64,
    title: String,
}

#[derive(Deserialize, Debug)]
struct RenameChatRequest {
    /** \brief 新的会话标题。 */
    title: String,
}

#[derive(Deserialize, Debug)]
struct HealthPreviewRequest {
    /** \brief 可选的显示名称。 */
    #[serde(default)]
    name: Option<String>,
    /** \brief Provider 类型。 */
    provider: String,
    /** \brief API 基地址。 */
    api_base: String,
    /** \brief API 密钥。 */
    api_key: String,
    /** \brief 默认模型名称。 */
    model: String,
}

fn build_provider_state(conn: &rusqlite::Connection) -> Result<ProvidersState, anyhow::Error> {
    let providers = db::list_providers(conn)?;
    let default_id = db::get_default_provider_id(conn)?;
    let telemetry_enabled = db::get_telemetry_enabled(conn)?;
    let items = providers
        .into_iter()
        .map(|p| ProviderItem {
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
    telemetry::set_enabled(telemetry_enabled);
    Ok(ProvidersState {
        providers: items,
        default_provider_id: default_id,
        telemetry_enabled,
    })
}

/**
 * \brief 获取当前默认 Provider 配置。
 */
async fn get_config() -> Result<Json<ProvidersState>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let state = build_provider_state(&conn).map_err(internal_err)?;
    Ok(Json(state))
}

/**
 * \brief 设置默认 Provider 配置。
 */
async fn set_config(
    Json(input): Json<ProviderInput>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let set_default = input.set_default.unwrap_or(true);
    let name = input.name.unwrap_or_else(|| "default".to_string());
    let id = if set_default {
        db::upsert_default_provider(
            &conn,
            &name,
            &input.provider,
            &input.api_base,
            &input.api_key,
            &input.model,
            None,
        )
        .map_err(internal_err)?
    } else {
        db::insert_provider(
            &conn,
            &name,
            &input.provider,
            &input.api_base,
            &input.api_key,
            &input.model,
            None,
        )
        .map_err(internal_err)?
    };
    if let Some(enabled) = input.telemetry_enabled {
        db::set_telemetry_enabled(&conn, enabled).map_err(internal_err)?;
        telemetry::set_enabled(enabled);
    }
    Ok(Json(serde_json::json!({"id": id})))
}

/**
 * \brief 获取 Provider 列表。
 */
async fn get_providers() -> Result<Json<ProvidersState>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let state = build_provider_state(&conn).map_err(internal_err)?;
    Ok(Json(state))
}

/**
 * \brief 新增 Provider。
 */
async fn create_provider(
    Json(payload): Json<ProviderRequest>,
) -> Result<Json<ProvidersState>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let set_default = payload.set_default.unwrap_or(false);
    if let Some(enabled) = payload.telemetry_enabled {
        db::set_telemetry_enabled(&conn, enabled).map_err(internal_err)?;
        telemetry::set_enabled(enabled);
    }
    if set_default {
        db::upsert_default_provider(
            &conn,
            &payload.name,
            &payload.provider,
            &payload.api_base,
            &payload.api_key,
            &payload.model,
            None,
        )
        .map_err(internal_err)?;
    } else {
        db::insert_provider(
            &conn,
            &payload.name,
            &payload.provider,
            &payload.api_base,
            &payload.api_key,
            &payload.model,
            None,
        )
        .map_err(internal_err)?;
    }
    telemetry::log_event(
        "server.provider",
        &format!("create name={} type={}", payload.name, payload.provider),
    );
    let state = build_provider_state(&conn).map_err(internal_err)?;
    Ok(Json(state))
}

/**
 * \brief 更新 Provider。
 */
async fn update_provider(
    Path(id): Path<i64>,
    Json(payload): Json<ProviderRequest>,
) -> Result<Json<ProvidersState>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    db::update_provider(
        &conn,
        id,
        &payload.name,
        &payload.provider,
        &payload.api_base,
        &payload.api_key,
        &payload.model,
        None,
    )
    .map_err(internal_err)?;
    if payload.set_default.unwrap_or(false) {
        db::set_default_provider_id(&conn, id).map_err(internal_err)?;
    }
    if let Some(enabled) = payload.telemetry_enabled {
        db::set_telemetry_enabled(&conn, enabled).map_err(internal_err)?;
        telemetry::set_enabled(enabled);
    }
    telemetry::log_event(
        "server.provider",
        &format!("update id={} name={}", id, payload.name),
    );
    let state = build_provider_state(&conn).map_err(internal_err)?;
    Ok(Json(state))
}

/**
 * \brief 删除 Provider。
 */
async fn delete_provider(
    Path(id): Path<i64>,
) -> Result<Json<ProvidersState>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    db::delete_provider(&conn, id).map_err(internal_err)?;
    telemetry::log_event("server.provider", &format!("delete id={}", id));
    let state = build_provider_state(&conn).map_err(internal_err)?;
    Ok(Json(state))
}

/**
 * \brief 设置默认 Provider。
 */
async fn select_provider(
    Path(id): Path<i64>,
) -> Result<Json<ProvidersState>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    db::set_default_provider_id(&conn, id).map_err(internal_err)?;
    telemetry::log_event("server.provider", &format!("select-default id={}", id));
    let state = build_provider_state(&conn).map_err(internal_err)?;
    Ok(Json(state))
}

/**
 * \brief 列出历史会话。
 */
async fn list_chats(
    Query(q): Query<ChatListQuery>,
) -> Result<Json<ChatListResponse>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let chats = db::list_chats(&conn, q.provider_id).map_err(internal_err)?;
    let items = chats
        .into_iter()
        .map(|c| ChatSummaryDto {
            id: c.id,
            title: c.title,
            provider_id: c.provider_id,
        })
        .collect();
    Ok(Json(ChatListResponse { chats: items }))
}

/**
 * \brief 获取指定会话的消息。
 */
async fn get_chat_messages(
    Path(id): Path<i64>,
) -> Result<Json<ChatMessagesResponse>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let provider = db::get_provider_for_chat(&conn, id).map_err(internal_err)?;
    let provider_id = provider.as_ref().map(|p| p.id);
    let messages = db::load_messages_with_meta(&conn, id).map_err(internal_err)?;
    let payload = messages
        .into_iter()
        .map(|m| ChatMessageDto {
            id: m.id,
            role: m.role,
            content: m.content,
        })
        .collect();
    Ok(Json(ChatMessagesResponse {
        chat_id: id,
        provider_id,
        messages: payload,
    }))
}

/**
 * \brief 删除指定会话。
 */
async fn remove_chat(
    Path(id): Path<i64>,
) -> Result<Json<ChatListResponse>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    db::delete_chat(&conn, id).map_err(internal_err)?;
    telemetry::log_event("server.chat", &format!("delete chat id={}", id));
    let chats = db::list_chats(&conn, None).map_err(internal_err)?;
    let items = chats
        .into_iter()
        .map(|c| ChatSummaryDto {
            id: c.id,
            title: c.title,
            provider_id: c.provider_id,
        })
        .collect();
    Ok(Json(ChatListResponse { chats: items }))
}

/**
 * \brief 重命名指定会话。
 */
async fn rename_chat(
    Path(id): Path<i64>,
    Json(payload): Json<RenameChatRequest>,
) -> Result<Json<ChatSummaryDto>, (axum::http::StatusCode, String)> {
    let trimmed_title = payload.title.trim();
    if trimmed_title.is_empty() {
        return Err(internal_err(anyhow!("会话标题不能为空")));
    }

    let conn = db::open_default_db().map_err(internal_err)?;
    db::update_chat_title(&conn, id, trimmed_title).map_err(internal_err)?;
    let provider = db::get_provider_for_chat(&conn, id).map_err(internal_err)?;
    telemetry::log_event(
        "server.chat",
        &format!("rename chat id={} title={}", id, trimmed_title),
    );

    Ok(Json(ChatSummaryDto {
        id,
        title: trimmed_title.to_string(),
        provider_id: provider.map(|p| p.id),
    }))
}

/**
 * \brief 克隆聊天并可选截断至指定消息。
 */
async fn branch_chat(
    Path(id): Path<i64>,
    Json(payload): Json<BranchRequest>,
) -> Result<Json<BranchResponse>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let title = payload.title.unwrap_or_else(|| format!("Chat {} 分支", id));
    let new_chat_id =
        db::clone_chat_until(&conn, id, &title, payload.until_message_id).map_err(internal_err)?;
    telemetry::log_event(
        "server.chat",
        &format!(
            "branch chat={} -> new_chat={} until={:?}",
            id, new_chat_id, payload.until_message_id
        ),
    );
    Ok(Json(BranchResponse {
        chat_id: new_chat_id,
        title,
    }))
}

#[derive(Deserialize, Debug)]
struct ChatQuery {
    /** \brief 会话ID（可选） */
    chat_id: Option<i64>,
    /** \brief Provider ID（可选） */
    provider_id: Option<i64>,
    /** \brief 用户发送的消息 */
    prompt: String,
    /** \brief 是否以流式返回（默认 true） */
    stream: Option<bool>,
    /** \brief 开启调试（默认 false），将推送 log 事件 */
    debug: Option<bool>,
    /** \brief 需要重新生成的消息 ID（针对助手消息）。 */
    regen_message_id: Option<i64>,
}

/**
 * \brief 聊天 SSE 流接口：GET /api/chat/sse?prompt=...&chat_id=...
 */
async fn chat_sse(
    Query(q): Query<ChatQuery>,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>,
    (axum::http::StatusCode, String),
> {
    if q.regen_message_id.is_some() && !q.prompt.trim().is_empty() {
        return Err(internal_err(anyhow!(
            "prompt 与 regen_message_id 不可同时提供"
        )));
    }

    let conn = db::open_default_db().map_err(internal_err)?;
    let telemetry_enabled = db::get_telemetry_enabled(&conn).map_err(internal_err)?;
    telemetry::set_enabled(telemetry_enabled);

    let mut provider_opt = None;
    if let Some(chat_id) = q.chat_id {
        if let Some(existing) = db::get_provider_for_chat(&conn, chat_id).map_err(internal_err)? {
            provider_opt = Some(existing);
        }
    }
    if provider_opt.is_none() {
        if let Some(pid) = q.provider_id {
            provider_opt = db::get_provider_by_id(&conn, pid).map_err(internal_err)?;
        }
    }
    if provider_opt.is_none() {
        provider_opt = db::get_default_provider(&conn).map_err(internal_err)?;
    }
    let provider = provider_opt
        .ok_or_else(|| internal_err(anyhow!("尚未设置可用的模型服务，请先创建或选择模型服务")))?;

    let chat_id = match q.chat_id {
        Some(id) => {
            let current = db::get_provider_for_chat(&conn, id).map_err(internal_err)?;
            if current.as_ref().map(|p| p.id) != Some(provider.id) {
                db::set_chat_provider(&conn, id, Some(provider.id)).map_err(internal_err)?;
            }
            id
        }
        None => {
            if q.regen_message_id.is_some() {
                return Err(internal_err(anyhow!("重新生成需要现有会话 ID")));
            }
            db::create_chat(&conn, &format!("{} 会话", provider.name), provider.id)
                .map_err(internal_err)?
        }
    };

    if let Some(message_id) = q.regen_message_id {
        let metas = db::load_messages_with_meta(&conn, chat_id).map_err(internal_err)?;
        let target = metas
            .iter()
            .find(|m| m.id == message_id)
            .ok_or_else(|| internal_err(anyhow!("待重新生成的消息不存在")))?;
        if target.role != "assistant" {
            return Err(internal_err(anyhow!("仅支持对助手消息重新生成")));
        }
        db::delete_messages_from(&conn, chat_id, message_id).map_err(internal_err)?;
    } else {
        db::insert_message(&conn, chat_id, "user", &q.prompt).map_err(internal_err)?;
    }

    let messages = db::load_messages(&conn, chat_id).map_err(internal_err)?;

    let (tx, rx) = mpsc::unbounded_channel::<Result<Event, Infallible>>();
    let _ = tx.send(Ok(Event::default()
        .event("meta")
        .data(serde_json::json!({ "chat_id": chat_id }).to_string())));

    let debug = q.debug.unwrap_or(false);
    let stream_flag = q.stream.unwrap_or(true);
    let regen_flag = q.regen_message_id.is_some();
    let prompt_len = if regen_flag { 0 } else { q.prompt.len() };

    tokio::spawn(async move {
        if debug {
            let _ = tx.send(Ok(Event::default().event("log").data(format!(
                "request -> provider={} type={} base={} model={} chat_id={} msgs={}",
                provider.name,
                provider.provider_type,
                provider.api_base,
                provider.model,
                chat_id,
                messages.len()
            ))));
        }

        let mut assistant_buf = String::new();
        telemetry::log_event(
            "server.chat",
            &format!(
                "provider={}({}) chat_id={} action={} prompt_len={}",
                provider.name,
                provider.provider_type,
                chat_id,
                if regen_flag { "regenerate" } else { "send" },
                prompt_len
            ),
        );

        if stream_flag {
            match llm::stream_chat(&provider, &messages).await {
                Ok(mut s) => {
                    use futures_util::StreamExt;
                    while let Some(item) = s.as_mut().next().await {
                        match item {
                            Ok(delta) => {
                                assistant_buf.push_str(&delta);
                                let _ = tx.send(Ok(Event::default().data(delta)));
                            }
                            Err(e) => {
                                telemetry::log_error(
                                    "server.chat",
                                    &format!("stream error: {}", e),
                                );
                                let _ = tx.send(Ok(Event::default()
                                    .event("error")
                                    .data(format!("{}", e))));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    telemetry::log_error("server.chat", &format!("stream failed: {}", e));
                    let _ = tx.send(Ok(Event::default()
                        .event("error")
                        .data(format!("stream failed: {}", e))));
                }
            }
        } else {
            match llm::chat_once(&provider, &messages).await {
                Ok(full) => {
                    assistant_buf.push_str(&full);
                    let _ = tx.send(Ok(Event::default().data(full)));
                }
                Err(e) => {
                    telemetry::log_error("server.chat", &format!("chat_once failed: {}", e));
                    let _ = tx.send(Ok(Event::default().event("error").data(format!("{}", e))));
                }
            }
        }

        if !assistant_buf.is_empty() {
            if let Ok(conn2) = db::open_default_db() {
                let _ = db::insert_message(&conn2, chat_id, "assistant", &assistant_buf);
            }
        }
    });

    let stream = UnboundedReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new()))
}

fn internal_err<E: std::fmt::Display>(e: E) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn list_models(
    Query(q): Query<ModelQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let provider = if let Some(pid) = q.provider_id {
        db::get_provider_by_id(&conn, pid).map_err(internal_err)?
    } else {
        db::get_default_provider(&conn).map_err(internal_err)?
    };
    let provider = provider.ok_or_else(|| internal_err(anyhow!("no provider available")))?;
    let telemetry_enabled = db::get_telemetry_enabled(&conn).map_err(internal_err)?;
    telemetry::set_enabled(telemetry_enabled);
    let models = llm::list_models(&provider).await.map_err(internal_err)?;
    Ok(Json(serde_json::json!({"models": models})))
}

/**
 * \brief 健康检查：尝试列出模型并返回状态。
 */
async fn health_check(
    Query(q): Query<ModelQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let provider = if let Some(pid) = q.provider_id {
        db::get_provider_by_id(&conn, pid).map_err(internal_err)?
    } else {
        db::get_default_provider(&conn).map_err(internal_err)?
    };
    let provider = provider.ok_or_else(|| internal_err(anyhow!("no provider available")))?;
    let telemetry_enabled = db::get_telemetry_enabled(&conn).map_err(internal_err)?;
    telemetry::set_enabled(telemetry_enabled);
    match llm::list_models(&provider).await {
        Ok(list) => Ok(Json(serde_json::json!({
            "ok": true,
            "provider_id": provider.id,
            "provider": provider.provider_type,
            "base": provider.api_base,
            "model": provider.model,
            "models": list.len()
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "ok": false,
            "provider_id": provider.id,
            "provider": provider.provider_type,
            "base": provider.api_base,
            "model": provider.model,
            "error": e.to_string()
        }))),
    }
}

/**
 * \brief 健康检查预检：使用未保存的 Provider 配置进行验证。
 */
async fn health_check_preview(
    Json(payload): Json<HealthPreviewRequest>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let conn = db::open_default_db().map_err(internal_err)?;
    let telemetry_enabled = db::get_telemetry_enabled(&conn).map_err(internal_err)?;
    telemetry::set_enabled(telemetry_enabled);

    let provider = Provider {
        id: -1,
        name: payload
            .name
            .unwrap_or_else(|| "临时健康检查".to_string()),
        api_base: payload.api_base,
        api_key: payload.api_key,
        model: payload.model,
        provider_type: payload.provider,
        secret_alias: None,
    };

    match llm::list_models(&provider).await {
        Ok(list) => Ok(Json(serde_json::json!({
            "ok": true,
            "provider_id": provider.id,
            "provider": provider.provider_type,
            "base": provider.api_base,
            "model": provider.model,
            "models": list.len()
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "ok": false,
            "provider_id": provider.id,
            "provider": provider.provider_type,
            "base": provider.api_base,
            "model": provider.model,
            "error": e.to_string()
        }))),
    }
}
