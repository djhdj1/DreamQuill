use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use std::{thread, time::Duration};

use crate::models::{Message as ChatMessage, Provider};

#[derive(Debug, Clone)]
pub struct ChatSummary {
    pub id: i64,
    pub title: String,
    pub provider_id: Option<i64>,
}

/**
 * \brief 带主键的消息结构。
 */
#[derive(Debug, Clone)]
pub struct StoredMessage {
    /** \brief 消息行主键。 */
    pub id: i64,
    /** \brief 消息角色。 */
    pub role: String,
    /** \brief 消息正文。 */
    pub content: String,
}

/**
 * \brief 打开默认数据库文件（本地目录下的 dreamquill.db）。
 */
pub fn open_default_db() -> Result<Connection> {
    let conn = Connection::open("dreamquill.db")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(conn)
}

/**
 * \brief 运行数据库迁移，创建必要表结构。
 */
pub fn migrate(conn: &Connection) -> Result<()> {
    retry_on_locked(|| {
        conn.execute_batch(
            r#"
        PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS providers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            api_base TEXT NOT NULL,
            api_key  TEXT NOT NULL,
            model    TEXT NOT NULL,
            provider_type TEXT NOT NULL DEFAULT 'openai',
            secret_alias TEXT
        );

        CREATE TABLE IF NOT EXISTS app_config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            provider_id INTEGER REFERENCES providers(id)
        );

        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            chat_id INTEGER NOT NULL REFERENCES chats(id),
            role TEXT NOT NULL,
            content TEXT NOT NULL
        );
        "#,
        )
    })?;

    ensure_provider_type_column(conn)?;
    ensure_provider_name_column(conn)?;
    ensure_chats_provider_nullable(conn)?;
    ensure_provider_secret_alias_column(conn)?;
    Ok(())
}

fn ensure_provider_type_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(providers)")?;
    let mut rows = stmt.query([])?;
    let mut has = false;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == "provider_type" {
            has = true;
            break;
        }
    }
    if !has {
        retry_on_locked(|| {
            conn.execute(
                "ALTER TABLE providers ADD COLUMN provider_type TEXT NOT NULL DEFAULT 'openai'",
                [],
            )
        })?;
    }
    Ok(())
}

fn ensure_provider_name_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(providers)")?;
    let mut rows = stmt.query([])?;
    let mut has = false;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == "name" {
            has = true;
            break;
        }
    }
    if !has {
        retry_on_locked(|| {
            conn.execute(
                "ALTER TABLE providers ADD COLUMN name TEXT NOT NULL DEFAULT 'default'",
                [],
            )
        })?;
    }
    Ok(())
}

fn ensure_provider_secret_alias_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(providers)")?;
    let mut rows = stmt.query([])?;
    let mut has = false;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == "secret_alias" {
            has = true;
            break;
        }
    }
    if !has {
        retry_on_locked(|| conn.execute("ALTER TABLE providers ADD COLUMN secret_alias TEXT", []))?;
    }
    Ok(())
}

fn ensure_chats_provider_nullable(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(chats)")?;
    let mut rows = stmt.query([])?;
    let mut needs_migration = false;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == "provider_id" {
            let not_null: i64 = row.get(3)?;
            if not_null != 0 {
                needs_migration = true;
                break;
            }
        }
    }
    if needs_migration {
        retry_on_locked(|| {
            conn.execute_batch(
                r#"
            PRAGMA foreign_keys=OFF;
            DROP TABLE IF EXISTS chats_tmp;
            CREATE TABLE chats_tmp (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                provider_id INTEGER REFERENCES providers(id)
            );
            INSERT INTO chats_tmp (id, title, provider_id)
                SELECT id, title, provider_id FROM chats;
            DROP TABLE chats;
            ALTER TABLE chats_tmp RENAME TO chats;
            PRAGMA foreign_keys=ON;
            "#,
            )
        })?;
    }
    Ok(())
}

fn set_bool_config(conn: &Connection, key: &str, value: bool) -> Result<()> {
    retry_on_locked(|| {
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, if value { "1" } else { "0" }],
        )
    })?;
    Ok(())
}

fn get_bool_config(conn: &Connection, key: &str, default: bool) -> Result<bool> {
    let val = conn
        .query_row(
            "SELECT value FROM app_config WHERE key=?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(val.map(|s| s == "1").unwrap_or(default))
}

/**
 * \brief 新增 Provider。
 */
pub fn insert_provider(
    conn: &Connection,
    name: &str,
    provider_type: &str,
    api_base: &str,
    api_key: &str,
    model: &str,
    secret_alias: Option<&str>,
) -> Result<i64> {
    retry_on_locked(|| {
        conn.execute(
            "INSERT INTO providers (name, api_base, api_key, model, provider_type, secret_alias) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![name, api_base, api_key, model, provider_type, secret_alias],
        )
    })?;
    Ok(conn.last_insert_rowid())
}

/**
 * \brief 更新 Provider。
 */
pub fn update_provider(
    conn: &Connection,
    id: i64,
    name: &str,
    provider_type: &str,
    api_base: &str,
    api_key: &str,
    model: &str,
    secret_alias: Option<&str>,
) -> Result<()> {
    let rows = retry_on_locked(|| {
        conn.execute(
            "UPDATE providers SET name=?1, provider_type=?2, api_base=?3, api_key=?4, model=?5, secret_alias=?6 WHERE id=?7",
            params![name, provider_type, api_base, api_key, model, secret_alias, id],
        )
    })?;
    if rows == 0 {
        bail!("provider id {} not found", id);
    }
    Ok(())
}

/**
 * \brief 删除 Provider（若存在关联会话则失败）。
 */
pub fn delete_provider(conn: &Connection, id: i64) -> Result<()> {
    if let Some(default_id) = get_default_provider_id(conn)? {
        if default_id == id {
            clear_default_provider(conn)?;
        }
    }

    retry_on_locked(|| {
        conn.execute(
            "UPDATE chats SET provider_id=NULL WHERE provider_id=?1",
            params![id],
        )
    })?;

    retry_on_locked(|| conn.execute("DELETE FROM providers WHERE id=?1", params![id]))?;
    Ok(())
}

/**
 * \brief 更新指定 Provider 的安全存储别名。
 */
pub fn set_provider_secret_alias(conn: &Connection, id: i64, alias: Option<&str>) -> Result<()> {
    retry_on_locked(|| {
        conn.execute(
            "UPDATE providers SET secret_alias=?1 WHERE id=?2",
            params![alias, id],
        )
    })?;
    Ok(())
}

/**
 * \brief 列出所有 Provider。
 */
pub fn list_providers(conn: &Connection) -> Result<Vec<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, api_base, api_key, model, provider_type, secret_alias FROM providers ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Provider {
                id: row.get(0)?,
                name: row.get(1)?,
                api_base: row.get(2)?,
                api_key: row.get(3)?,
                model: row.get(4)?,
                provider_type: row.get(5)?,
                secret_alias: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/**
 * \brief 设置默认 Provider。
 */
pub fn set_default_provider_id(conn: &Connection, id: i64) -> Result<()> {
    if get_provider_by_id(conn, id)?.is_none() {
        bail!("provider id {} not found", id);
    }
    retry_on_locked(|| {
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('default_provider_id', ?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![id.to_string()],
        )
    })?;
    Ok(())
}

fn clear_default_provider(conn: &Connection) -> Result<()> {
    retry_on_locked(|| conn.execute("DELETE FROM app_config WHERE key='default_provider_id'", []))?;
    Ok(())
}

pub fn get_default_provider_id(conn: &Connection) -> Result<Option<i64>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key='default_provider_id'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(id.and_then(|s| s.parse::<i64>().ok()))
}

/**
 * \brief 读取默认 Provider（若未设置，返回 None）。
 */
pub fn get_default_provider(conn: &Connection) -> Result<Option<Provider>> {
    if let Some(id) = get_default_provider_id(conn)? {
        get_provider_by_id(conn, id)
    } else {
        Ok(None)
    }
}

/**
 * \brief 按 ID 获取 Provider。
 */
pub fn get_provider_by_id(conn: &Connection, id: i64) -> Result<Option<Provider>> {
    conn
        .query_row(
            "SELECT id, name, api_base, api_key, model, provider_type, secret_alias FROM providers WHERE id=?1",
            params![id],
            |row| {
                Ok(Provider {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    api_base: row.get(2)?,
                    api_key: row.get(3)?,
                    model: row.get(4)?,
                    provider_type: row.get(5)?,
                    secret_alias: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

/**
 * \brief 创建 Provider 并设为默认。
 */
pub fn upsert_default_provider(
    conn: &Connection,
    name: &str,
    provider_type: &str,
    api_base: &str,
    api_key: &str,
    model: &str,
    secret_alias: Option<&str>,
) -> Result<i64> {
    let id = insert_provider(
        conn,
        name,
        provider_type,
        api_base,
        api_key,
        model,
        secret_alias,
    )?;
    set_default_provider_id(conn, id)?;
    Ok(id)
}

/**
 * \brief 读取遥测开关。
 */
pub fn get_telemetry_enabled(conn: &Connection) -> Result<bool> {
    get_bool_config(conn, "telemetry_enabled", false)
}

/**
 * \brief 更新遥测开关。
 */
pub fn set_telemetry_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    set_bool_config(conn, "telemetry_enabled", enabled)
}

/**
 * \brief 创建会话。
 */
pub fn create_chat(conn: &Connection, title: &str, provider_id: i64) -> Result<i64> {
    retry_on_locked(|| {
        conn.execute(
            "INSERT INTO chats (title, provider_id) VALUES (?1, ?2)",
            params![title, provider_id],
        )
    })?;
    Ok(conn.last_insert_rowid())
}

/**
 * \brief 插入一条消息。
 */
pub fn insert_message(conn: &Connection, chat_id: i64, role: &str, content: &str) -> Result<i64> {
    retry_on_locked(|| {
        conn.execute(
            "INSERT INTO messages (chat_id, role, content) VALUES (?1, ?2, ?3)",
            params![chat_id, role, content],
        )
    })?;
    Ok(conn.last_insert_rowid())
}

/**
 * \brief 读取指定会话的全部消息（简单实现，M1）。
 */
pub fn load_messages(conn: &Connection, chat_id: i64) -> Result<Vec<ChatMessage>> {
    let mut stmt =
        conn.prepare("SELECT role, content FROM messages WHERE chat_id=?1 ORDER BY id ASC")?;
    let rows = stmt
        .query_map(params![chat_id], |row| {
            Ok(ChatMessage {
                role: row.get(0)?,
                content: row.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/**
 * \brief 读取带主键的消息数组，用于前端展示与高级操作。
 */
pub fn load_messages_with_meta(conn: &Connection, chat_id: i64) -> Result<Vec<StoredMessage>> {
    let mut stmt =
        conn.prepare("SELECT id, role, content FROM messages WHERE chat_id=?1 ORDER BY id ASC")?;
    let rows = stmt
        .query_map(params![chat_id], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/**
 * \brief 获取指定会话的 Provider。
 */
pub fn get_provider_for_chat(conn: &Connection, chat_id: i64) -> Result<Option<Provider>> {
    let provider_id: Option<i64> = conn
        .query_row(
            "SELECT provider_id FROM chats WHERE id=?1",
            params![chat_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(pid) = provider_id {
        get_provider_by_id(conn, pid)
    } else {
        Ok(None)
    }
}

/**
 * \brief 为指定会话更新模型服务关联。
 */
pub fn set_chat_provider(conn: &Connection, chat_id: i64, provider_id: Option<i64>) -> Result<()> {
    retry_on_locked(|| {
        conn.execute(
            "UPDATE chats SET provider_id=?1 WHERE id=?2",
            params![provider_id, chat_id],
        )
    })?;
    Ok(())
}

/**
 * \brief 列出指定 Provider 的会话列表。
 */
pub fn list_chats(conn: &Connection, provider_id: Option<i64>) -> Result<Vec<ChatSummary>> {
    fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatSummary> {
        Ok(ChatSummary {
            id: row.get(0)?,
            title: row.get(1)?,
            provider_id: row.get::<_, Option<i64>>(2)?,
        })
    }

    let mut results = Vec::new();

    if let Some(pid) = provider_id {
        let mut stmt = conn.prepare(
            "SELECT id, title, provider_id FROM chats WHERE provider_id=?1 ORDER BY id DESC",
        )?;
        let rows = stmt.query_map(params![pid], map_row)?;
        for row in rows {
            results.push(row?);
        }
    } else {
        let mut stmt = conn.prepare("SELECT id, title, provider_id FROM chats ORDER BY id DESC")?;
        let rows = stmt.query_map([], map_row)?;
        for row in rows {
            results.push(row?);
        }
    }

    Ok(results)
}

/**
 * \brief 删除指定会话及其消息。
 */
pub fn delete_chat(conn: &Connection, chat_id: i64) -> Result<()> {
    retry_on_locked(|| conn.execute("DELETE FROM messages WHERE chat_id=?1", params![chat_id]))?;
    retry_on_locked(|| conn.execute("DELETE FROM chats WHERE id=?1", params![chat_id]))?;
    Ok(())
}

/**
 * \brief 更新会话标题。
 */
pub fn update_chat_title(conn: &Connection, chat_id: i64, title: &str) -> Result<()> {
    let rows = retry_on_locked(|| {
        conn.execute(
            "UPDATE chats SET title=?1 WHERE id=?2",
            params![title, chat_id],
        )
    })?;
    if rows == 0 {
        bail!("chat id {} not found", chat_id);
    }
    Ok(())
}

/**
 * \brief 删除指定消息及之后的所有消息。
 */
pub fn delete_messages_from(conn: &Connection, chat_id: i64, from_message_id: i64) -> Result<()> {
    retry_on_locked(|| {
        conn.execute(
            "DELETE FROM messages WHERE chat_id=?1 AND id>=?2",
            params![chat_id, from_message_id],
        )
    })?;
    Ok(())
}

/**
 * \brief 克隆聊天记录到新会话，可选截断到指定消息。
 */
pub fn clone_chat_until(
    conn: &Connection,
    source_chat_id: i64,
    title: &str,
    until_message_id: Option<i64>,
) -> Result<i64> {
    let provider = get_provider_for_chat(conn, source_chat_id)?;
    let provider_id = provider
        .map(|p| p.id)
        .ok_or_else(|| anyhow!("source chat has no provider"))?;
    let new_chat_id = create_chat(conn, title, provider_id)?;
    let messages = load_messages_with_meta(conn, source_chat_id)?;
    for message in messages {
        if let Some(limit) = until_message_id {
            if message.id > limit {
                break;
            }
        }
        insert_message(conn, new_chat_id, &message.role, &message.content)?;
    }
    Ok(new_chat_id)
}

/**
 * \brief 针对 SQLite 锁冲突的重试助手。
 * \details 捕获 `database is locked`/`database table is locked` 等错误并进行指数退避，最大尝试 6 次。
 */
fn retry_on_locked<T, F>(mut action: F) -> Result<T>
where
    F: FnMut() -> rusqlite::Result<T>,
{
    const MAX_RETRIES: usize = 5;
    for attempt in 0..=MAX_RETRIES {
        match action() {
            Ok(value) => return Ok(value),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(
                    err.code,
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                ) && attempt < MAX_RETRIES =>
            {
                let backoff = Duration::from_millis(200 * (attempt as u64 + 1));
                thread::sleep(backoff);
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
    unreachable!("retry_on_locked should have returned within the loop");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        migrate(&conn).expect("migrate");
        conn
    }

    #[test]
    fn test_provider_crud_and_default() {
        let conn = mem_conn();
        let id1 = insert_provider(
            &conn,
            "p1",
            "openai",
            "https://api.example.com",
            "sk-1",
            "gpt-4o",
            None,
        )
        .expect("insert provider 1");
        let id2 = insert_provider(
            &conn,
            "p2",
            "openai",
            "https://api.example.com",
            "sk-2",
            "gpt-4o-mini",
            None,
        )
        .expect("insert provider 2");
        let list = list_providers(&conn).expect("list providers");
        assert_eq!(list.len(), 2);

        set_default_provider_id(&conn, id2).expect("set default");
        let def = get_default_provider(&conn).expect("get default");
        assert_eq!(def.unwrap().id, id2);

        update_provider(
            &conn,
            id1,
            "p1-up",
            "openai",
            "https://api.example.com",
            "",
            "gpt-4o",
            Some("alias-1"),
        )
        .expect("update provider");

        let one = get_provider_by_id(&conn, id1).expect("get by id").unwrap();
        assert_eq!(one.name, "p1-up");
        assert_eq!(one.secret_alias.as_deref(), Some("alias-1"));
    }

    #[test]
    fn test_chat_and_messages() {
        let conn = mem_conn();
        let pid = insert_provider(
            &conn,
            "p1",
            "openai",
            "https://api.example.com",
            "sk",
            "gpt",
            None,
        )
        .expect("insert provider");
        let chat_id = create_chat(&conn, "test chat", pid).expect("create chat");
        insert_message(&conn, chat_id, "user", "hello").expect("insert msg");
        insert_message(&conn, chat_id, "assistant", "hi").expect("insert msg");
        let msgs = load_messages(&conn, chat_id).expect("load msgs");
        assert_eq!(msgs.len(), 2);

        let chats = list_chats(&conn, Some(pid)).expect("list chats");
        assert_eq!(chats.len(), 1);

        delete_chat(&conn, chat_id).expect("delete chat");
        let chats = list_chats(&conn, Some(pid)).expect("list chats 2");
        assert_eq!(chats.len(), 0);
    }

    #[test]
    fn test_delete_messages_from_prunes_tail() {
        let conn = mem_conn();
        let pid = insert_provider(
            &conn,
            "p1",
            "openai",
            "https://api.example.com",
            "sk",
            "gpt",
            None,
        )
        .expect("insert provider");
        let chat_id = create_chat(&conn, "test chat", pid).expect("create chat");
        let first_id = insert_message(&conn, chat_id, "user", "hello").expect("insert 1");
        let second_id = insert_message(&conn, chat_id, "assistant", "hi").expect("insert 2");
        let _third_id = insert_message(&conn, chat_id, "user", "second turn").expect("insert 3");

        delete_messages_from(&conn, chat_id, second_id).expect("delete tail");
        let messages = load_messages_with_meta(&conn, chat_id).expect("load messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, first_id);
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn test_delete_messages_from_with_nonexistent_id_noop() {
        let conn = mem_conn();
        let pid = insert_provider(
            &conn,
            "p1",
            "openai",
            "https://api.example.com",
            "sk",
            "gpt",
            None,
        )
        .expect("insert provider");
        let chat_id = create_chat(&conn, "test chat", pid).expect("create chat");
        let first_id = insert_message(&conn, chat_id, "user", "hello").expect("insert 1");
        let second_id = insert_message(&conn, chat_id, "assistant", "hi").expect("insert 2");
        let third_id = insert_message(&conn, chat_id, "user", "second turn").expect("insert 3");

        delete_messages_from(&conn, chat_id, third_id + 100).expect("delete noop");
        let messages = load_messages_with_meta(&conn, chat_id).expect("load messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].id, first_id);
        assert_eq!(messages[1].id, second_id);
        assert_eq!(messages[2].id, third_id);
    }

    #[test]
    fn test_clone_chat_until_copies_full_history() {
        let conn = mem_conn();
        let pid = insert_provider(
            &conn,
            "p1",
            "openai",
            "https://api.example.com",
            "sk",
            "gpt",
            None,
        )
        .expect("insert provider");
        let chat_id = create_chat(&conn, "original", pid).expect("create chat");
        insert_message(&conn, chat_id, "user", "hello").expect("insert 1");
        insert_message(&conn, chat_id, "assistant", "hi").expect("insert 2");
        insert_message(&conn, chat_id, "user", "follow up").expect("insert 3");

        let new_chat_id =
            clone_chat_until(&conn, chat_id, "branch all", None).expect("clone chat full");
        let messages = load_messages_with_meta(&conn, new_chat_id).expect("load cloned messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello");
        let provider = get_provider_for_chat(&conn, new_chat_id)
            .expect("get provider")
            .expect("provider exists");
        assert_eq!(provider.id, pid);
    }

    #[test]
    fn test_clone_chat_until_truncates_at_message() {
        let conn = mem_conn();
        let pid = insert_provider(
            &conn,
            "p1",
            "openai",
            "https://api.example.com",
            "sk",
            "gpt",
            None,
        )
        .expect("insert provider");
        let chat_id = create_chat(&conn, "original", pid).expect("create chat");
        let _first = insert_message(&conn, chat_id, "user", "hello").expect("insert 1");
        let second = insert_message(&conn, chat_id, "assistant", "hi").expect("insert 2");
        insert_message(&conn, chat_id, "user", "follow up").expect("insert 3");

        let new_chat_id =
            clone_chat_until(&conn, chat_id, "branch two", Some(second)).expect("clone truncated");
        let messages = load_messages_with_meta(&conn, new_chat_id).expect("load cloned messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "hi");
    }

    #[test]
    fn test_clone_chat_until_with_limit_before_first_message_creates_empty_history() {
        let conn = mem_conn();
        let pid = insert_provider(
            &conn,
            "p1",
            "openai",
            "https://api.example.com",
            "sk",
            "gpt",
            None,
        )
        .expect("insert provider");
        let chat_id = create_chat(&conn, "original", pid).expect("create chat");
        let first = insert_message(&conn, chat_id, "user", "hello").expect("insert 1");
        let limit = first - 1;

        let new_chat_id =
            clone_chat_until(&conn, chat_id, "empty branch", Some(limit)).expect("clone empty");
        let messages = load_messages_with_meta(&conn, new_chat_id).expect("load cloned messages");
        assert!(messages.is_empty());
    }

    #[test]
    fn test_clone_chat_until_without_provider_fails() {
        let conn = mem_conn();
        let pid = insert_provider(
            &conn,
            "p1",
            "openai",
            "https://api.example.com",
            "sk",
            "gpt",
            None,
        )
        .expect("insert provider");
        let chat_id = create_chat(&conn, "original", pid).expect("create chat");
        insert_message(&conn, chat_id, "user", "hello").expect("insert 1");
        set_chat_provider(&conn, chat_id, None).expect("clear provider");
        let result = clone_chat_until(&conn, chat_id, "branch", None);
        assert!(result.is_err());
    }
}
