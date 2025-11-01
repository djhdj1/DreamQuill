import type {
  BranchChatOptions,
  BranchResult,
  ChatMessagesPayload,
  ChatSummary,
  SendChatParams,
} from '../types';
import type { Transport, TransportStreamHandle } from '../transport';

/** @brief 聊天服务封装。 */
export class ChatService {
  constructor(private readonly transport: Transport) {}

  /** @brief 发起聊天并返回流式事件句柄。 */
  send(params: SendChatParams): TransportStreamHandle {
    return this.transport.stream({
      path: '/chat/sse',
      ...params,
    });
  }

  /** @brief 列出历史会话。 */
  async listChats(): Promise<ChatSummary[]> {
    const response = await this.transport.request<{
      chats: Array<{ id: number; title: string; provider_id: number | null }>;
    }>({
      method: 'GET',
      path: '/chats',
    });
    return (response.chats ?? []).map((item) => ({
      id: item.id,
      title: item.title,
      providerId: item.provider_id,
    }));
  }

  /** @brief 获取指定会话的消息。 */
  async getMessages(chatId: number): Promise<ChatMessagesPayload> {
    const response = await this.transport.request<{
      chat_id: number;
      provider_id: number | null;
      messages: Array<{ id: number; role: string; content: string }>;
    }>({
      method: 'GET',
      path: `/chats/${chatId}/messages`,
    });
    return {
      chatId: response.chat_id,
      providerId: response.provider_id,
      messages: (response.messages ?? []).map((msg) => ({
        id: msg.id,
        role: msg.role as ChatMessagesPayload['messages'][number]['role'],
        content: msg.content,
      })),
    };
  }

  /** @brief 删除会话并返回剩余会话列表。 */
  async deleteChat(chatId: number): Promise<ChatSummary[]> {
    const response = await this.transport.request<{
      chats: Array<{ id: number; title: string; provider_id: number | null }>;
    }>({
      method: 'DELETE',
      path: `/chats/${chatId}`,
    });
    return (response.chats ?? []).map((item) => ({
      id: item.id,
      title: item.title,
      providerId: item.provider_id,
    }));
  }

  /** @brief 针对会话创建分支。 */
  async branchChat(chatId: number, options: BranchChatOptions = {}): Promise<BranchResult> {
    const response = await this.transport.request<{ chat_id: number; title: string }>({
      method: 'POST',
      path: `/chats/${chatId}/branch`,
      body: {
        until_message_id: options.untilMessageId,
        title: options.title,
      },
    });
    return {
      chatId: response.chat_id,
      title: response.title,
    };
  }

  /** @brief 重命名会话标题。 */
  async renameChat(chatId: number, title: string): Promise<ChatSummary> {
    const response = await this.transport.request<{
      id: number;
      title: string;
      provider_id: number | null;
    }>({
      method: 'PUT',
      path: `/chats/${chatId}`,
      body: { title },
    });
    return {
      id: response.id,
      title: response.title,
      providerId: response.provider_id,
    };
  }
}
