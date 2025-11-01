use serde::{Deserialize, Serialize};

/**
 * \brief Provider 配置模型。
 */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    /** \brief 自增主键 */
    pub id: i64,
    /** \brief Provider 显示名称 */
    pub name: String,
    /** \brief API 基地址 */
    pub api_base: String,
    /** \brief API Key（明文存储，M1 阶段可接受，后续迁移至安全存储） */
    pub api_key: String,
    /** \brief 默认模型名 */
    pub model: String,
    /** \brief Provider 类型 */
    pub provider_type: String,
    /** \brief 关联安全存储的别名（若存在）。 */
    pub secret_alias: Option<String>,
}

/**
 * \brief 消息结构，与 OpenAI Chat 消息格式对齐。
 */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /** \brief 角色：system/user/assistant */
    pub role: String,
    /** \brief 内容 */
    pub content: String,
}
