import {
  createDreamQuillClient,
  type StreamEvent,
  type TransportStreamHandle,
  type HealthStatus,
} from '@dreamquill/ts-sdk';
import type {
  BranchChatOptions,
  BranchResult,
  ChatMessagesPayload,
  ChatSummary,
  ProviderConfig,
  ProviderState,
  SendChatCallbacks,
  SendChatParams,
} from './types';

const client = createDreamQuillClient();

/** @brief UI 层流式句柄。 */
export interface ChatStreamHandle {
  /** @brief 主动取消流。 */
  cancel(): void;
  /** @brief 当流结束时解析。 */
  completed: Promise<void>;
}

/** @brief 将流式事件转译为 UI 所需回调。 */
async function consumeStream(
  events: AsyncIterable<StreamEvent>,
  callbacks: SendChatCallbacks,
): Promise<void> {
  try {
    for await (const event of events) {
      switch (event.type) {
        case 'meta':
          callbacks.onMeta({ chatId: event.chatId });
          break;
        case 'chunk':
          callbacks.onChunk(event.text);
          break;
        case 'log':
          callbacks.onLog({ level: event.level, text: event.message });
          break;
        case 'error':
          callbacks.onError(event.message);
          callbacks.onLog({ level: 'error', text: event.message });
          break;
        default:
          callbacks.onLog({ level: 'log', text: `未知事件：${JSON.stringify(event)}` });
          break;
      }
    }
  } catch (error) {
    const message = `流式读取失败：${String(error)}`;
    callbacks.onError(message);
    callbacks.onLog({ level: 'error', text: message });
  }
}

function dispatchStream(handle: TransportStreamHandle, callbacks: SendChatCallbacks): ChatStreamHandle {
  const completed = consumeStream(handle.events, callbacks);
  return {
    cancel: () => handle.cancel(),
    completed,
  };
}

export async function fetchProviderState(): Promise<ProviderState> {
  return client.providers.fetchState();
}

export async function createProvider(
  config: ProviderConfig,
  options?: { setDefault?: boolean; telemetryEnabled?: boolean },
): Promise<ProviderState> {
  return client.providers.create(config, options);
}

export async function updateProvider(
  id: number,
  config: ProviderConfig,
  options?: { setDefault?: boolean; telemetryEnabled?: boolean },
): Promise<ProviderState> {
  return client.providers.update(id, config, options);
}

export async function deleteProvider(id: number): Promise<ProviderState> {
  return client.providers.remove(id);
}

export async function selectProvider(id: number): Promise<ProviderState> {
  return client.providers.selectDefault(id);
}

export async function listModels(providerId?: number): Promise<string[]> {
  return client.providers.listModels(providerId);
}

export async function healthCheck(providerId?: number): Promise<HealthStatus> {
  return client.providers.healthCheck(providerId);
}

/** @brief 使用暂存配置执行健康检查，不依赖已保存服务。 */
export async function healthCheckPreview(config: ProviderConfig): Promise<HealthStatus> {
  return client.providers.healthCheckPreview(config);
}

export async function sendChat(
  params: SendChatParams,
  callbacks: SendChatCallbacks,
): Promise<ChatStreamHandle> {
  const handle = client.chat.send(params);
  return dispatchStream(handle, callbacks);
}

export async function listChats(): Promise<ChatSummary[]> {
  return client.chat.listChats();
}

export async function fetchChatMessages(chatId: number): Promise<ChatMessagesPayload> {
  return client.chat.getMessages(chatId);
}

export async function deleteChat(chatId: number): Promise<ChatSummary[]> {
  return client.chat.deleteChat(chatId);
}

/** @brief 重命名会话标题。 */
export async function renameChat(chatId: number, title: string): Promise<ChatSummary> {
  return client.chat.renameChat(chatId, title);
}

export async function branchChat(
  chatId: number,
  options: BranchChatOptions = {},
): Promise<BranchResult> {
  return client.chat.branchChat(chatId, options);
}
