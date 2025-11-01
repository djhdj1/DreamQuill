import type * as Types from './types';

/** @brief 通用请求选项。 */
export interface TransportRequestOptions<TResponse = unknown> {
  /** @brief HTTP 动作。 */
  method: 'GET' | 'POST' | 'PUT' | 'DELETE';
  /** @brief 请求路径。 */
  path: string;
  /** @brief 查询参数集合。 */
  query?: Record<string, string | number | boolean | undefined>;
  /** @brief 请求体。 */
  body?: unknown;
  /** @brief 响应解析函数，可覆写默认 JSON 解析。 */
  parse?: (raw: unknown) => TResponse;
}

/** @brief 聊天流式请求选项。 */
export interface TransportStreamOptions extends Types.SendChatParams {
  /** @brief 兼容 HTTP 路径。 */
  path: string;
}

/** @brief 流式传输句柄。 */
export interface TransportStreamHandle {
  /** @brief 异步事件序列。 */
  events: AsyncIterable<Types.StreamEvent>;
  /** @brief 主动取消流。 */
  cancel(): void;
}

/** @brief 统一传输抽象，用于屏蔽 Tauri 与 HTTP 环境差异。 */
export interface Transport {
  /** @brief 普通请求。 */
  request<TResponse>(options: TransportRequestOptions<TResponse>): Promise<TResponse>;
  /** @brief 流式聊天请求。 */
  stream(options: TransportStreamOptions): TransportStreamHandle;
}
