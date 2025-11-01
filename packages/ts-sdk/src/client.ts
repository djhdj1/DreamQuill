import { HttpTransport } from './adapters/http';
import { TauriTransport } from './adapters/tauri';
import { ChatService } from './services/chat';
import { ProviderService } from './services/providers';
import type { RuntimeMode } from './types';
import type { Transport } from './transport';

/** @brief 客户端初始化选项。 */
export interface ClientOptions {
  /** @brief 运行模式，默认自动检测。 */
  mode?: RuntimeMode;
  /** @brief HTTP 模式下的基础路径。 */
  basePath?: string;
  /** @brief 自定义传输实现，用于测试或扩展。 */
  transport?: Transport;
}

/** @brief SDK 客户端实例。 */
export interface DreamQuillClient {
  /** @brief 底层传输层。 */
  transport: Transport;
  /** @brief Provider 领域服务。 */
  providers: ProviderService;
  /** @brief 聊天领域服务。 */
  chat: ChatService;
}

function detectMode(): RuntimeMode {
  if (typeof window !== 'undefined' && typeof (window as unknown as { __TAURI_IPC__?: unknown }).__TAURI_IPC__ !== 'undefined') {
    return 'tauri';
  }
  return 'http';
}

/** @brief 创建统一的 DreamQuill 客户端。 */
export function createDreamQuillClient(options: ClientOptions = {}): DreamQuillClient {
  const mode = options.mode ?? detectMode();
  const transport =
    options.transport ??
    (mode === 'tauri' ? new TauriTransport() : new HttpTransport(options.basePath ?? '/api'));

  return {
    transport,
    providers: new ProviderService(transport),
    chat: new ChatService(transport),
  };
}
