import type {
  ChatMessage as SdkChatMessage,
  StoredChatMessage as SdkStoredChatMessage,
  ChatSummary as SdkChatSummary,
  ProviderConfig as SdkProviderConfig,
  ProviderRecord as SdkProviderRecord,
  ProviderState as SdkProviderState,
  SendChatParams as SdkSendChatParams,
  BranchResult as SdkBranchResult,
  BranchChatOptions as SdkBranchChatOptions,
  ChatMessagesPayload as SdkChatMessagesPayload,
} from '@dreamquill/ts-sdk';

/** @brief Provider 基础配置。 */
export type ProviderConfig = SdkProviderConfig;

/** @brief Provider 记录。 */
export type ProviderRecord = SdkProviderRecord;

/** @brief Provider 状态。 */
export type ProviderState = SdkProviderState;

/** @brief 聊天消息实体。 */
export type ChatMessage = SdkChatMessage;

/** @brief 带主键的存储消息。 */
export type StoredChatMessage = SdkStoredChatMessage;

/** @brief 会话摘要。 */
export type ChatSummary = SdkChatSummary;

/** @brief 会话消息载体。 */
export type ChatMessagesPayload = SdkChatMessagesPayload;

/** @brief 会话分支结果。 */
export type BranchResult = SdkBranchResult;

/** @brief 会话分支参数。 */
export type BranchChatOptions = SdkBranchChatOptions;

/** @brief 调试日志消息。 */
export interface DebugLog {
  /** @brief 日志等级。 */
  level: 'info' | 'error' | 'log';
  /** @brief 日志正文。 */
  text: string;
}

/** @brief 发送消息参数。 */
export type SendChatParams = SdkSendChatParams;

/** @brief 发送消息时的回调集合。 */
export interface SendChatCallbacks {
  /** @brief 收到元事件。 */
  onMeta: (meta: { chatId: number }) => void;
  /** @brief 收到文本增量。 */
  onChunk: (chunk: string) => void;
  /** @brief 收到错误信息。 */
  onError: (message: string) => void;
  /** @brief 收到调试日志。 */
  onLog: (log: DebugLog) => void;
}
