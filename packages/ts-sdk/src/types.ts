/** @file 定义 SDK 对外暴露的核心类型。 */

/** @brief Provider 配置信息。 */
export interface ProviderConfig {
  /** @brief Provider 显示名称。 */
  name: string;
  /** @brief Provider 类型标识。 */
  provider: string;
  /** @brief API 基地址。 */
  apiBase: string;
  /** @brief API Key（仍需结合安全存储策略）。 */
  apiKey: string;
  /** @brief 默认模型名称。 */
  model: string;
}

/** @brief Provider 记录，附带 ID 与默认标记。 */
export interface ProviderRecord extends ProviderConfig {
  /** @brief Provider 主键 ID。 */
  id: number;
  /** @brief 是否为默认 Provider。 */
  isDefault: boolean;
}

/** @brief Provider 状态载体。 */
export interface ProviderState {
  /** @brief Provider 列表。 */
  providers: ProviderRecord[];
  /** @brief 默认 Provider ID。 */
  defaultProviderId: number | null;
  /** @brief 是否启用遥测。 */
  telemetryEnabled: boolean;
}

/** @brief 聊天消息实体。 */
export interface ChatMessage {
  /** @brief 消息所属角色。 */
  role: 'user' | 'assistant' | 'system' | 'tool';
  /** @brief 消息正文。 */
  content: string;
}

/** @brief 带主键的持久化消息。 */
export interface StoredChatMessage extends ChatMessage {
  /** @brief 消息主键。 */
  id: number;
}

/** @brief 聊天概要。 */
export interface ChatSummary {
  /** @brief 会话主键。 */
  id: number;
  /** @brief 会话标题。 */
  title: string;
  /** @brief 关联模型服务 ID。 */
  providerId: number | null;
}

/** @brief 发送聊天的参数。 */
export interface SendChatParams {
  /** @brief 现有会话 ID。 */
  chatId?: number;
  /** @brief 目标 Provider ID。 */
  providerId?: number;
  /** @brief 用户输入文本。 */
  prompt: string;
  /** @brief 是否请求流式响应。 */
  stream?: boolean;
  /** @brief 是否开启调试日志。 */
  debug?: boolean;
  /** @brief 针对助手消息的重新生成。 */
  regenMessageId?: number;
}

/** @brief 流式事件层级。 */
export type StreamEvent =
  | { type: 'meta'; chatId: number }
  | { type: 'chunk'; text: string }
  | { type: 'log'; level: 'info' | 'error' | 'log'; message: string }
  | { type: 'error'; message: string };

/** @brief SDK 可用模式。 */
export type RuntimeMode = 'http' | 'tauri';

/** @brief Provider 健康状态。 */
export interface HealthStatus {
  ok: boolean;
  providerId: number;
  provider?: string;
  base?: string;
  model?: string;
  models?: number;
  error?: string;
}

/** @brief 聊天分支操作结果。 */
export interface BranchResult {
  /** @brief 新会话 ID。 */
  chatId: number;
  /** @brief 新会话标题。 */
  title: string;
}

/** @brief 会话消息载体。 */
export interface ChatMessagesPayload {
  /** @brief 会话主键。 */
  chatId: number;
  /** @brief 绑定的模型服务 ID。 */
  providerId: number | null;
  /** @brief 消息数组。 */
  messages: StoredChatMessage[];
}

/** @brief 聊天分支参数。 */
export interface BranchChatOptions {
  /** @brief 截断到的消息 ID。 */
  untilMessageId?: number;
  /** @brief 新会话标题。 */
  title?: string;
}
