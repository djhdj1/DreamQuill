import type * as Types from '../types';
import type {
  Transport,
  TransportRequestOptions,
  TransportStreamHandle,
  TransportStreamOptions,
} from '../transport';

type InvokeFunc = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

let cachedInvoke: InvokeFunc | null = null;
type EventApi = {
  listen: <T = unknown>(event: string, handler: (ev: { payload: T }) => void) => Promise<() => void>;
};

let cachedEventApi: EventApi | null = null;

/** @brief 延迟加载 Tauri invoke，避免在非 Tauri 环境报错。 */
async function ensureInvoke(): Promise<InvokeFunc> {
  if (cachedInvoke) {
    return cachedInvoke;
  }
  const mod = await import('@tauri-apps/api/tauri');
  cachedInvoke = mod.invoke;
  return cachedInvoke!;
}

/** @brief 延迟加载 Tauri 事件 API。 */
async function ensureEventApi(): Promise<EventApi> {
  if (cachedEventApi) return cachedEventApi;
  const mod = await import('@tauri-apps/api/event');
  cachedEventApi = { listen: mod.listen } as EventApi;
  return cachedEventApi;
}

/** @brief Tauri 运行时传输适配器。 */
export class TauriTransport implements Transport {
  async request<TResponse>(options: TransportRequestOptions<TResponse>): Promise<TResponse> {
    const invoke = await ensureInvoke();
    const route = `${options.method} ${options.path}`;

    switch (true) {
      case route === 'GET /providers': {
        return invoke<TResponse>('dq_get_config');
      }
      case route === 'POST /providers': {
        return invoke<TResponse>('dq_create_provider', { payload: options.body });
      }
      case /^PUT \/providers\/\d+$/.test(route): {
        const id = Number(options.path.split('/')[2]);
        return invoke<TResponse>('dq_update_provider', { id, payload: options.body });
      }
      case /^DELETE \/providers\/\d+$/.test(route): {
        const id = Number(options.path.split('/')[2]);
        return invoke<TResponse>('dq_delete_provider', { id });
      }
      case /^POST \/providers\/\d+\/select$/.test(route): {
        const id = Number(options.path.split('/')[2]);
        return invoke<TResponse>('dq_select_provider', { id });
      }
      case route === 'GET /models': {
        const providerId = options.query?.provider_id;
        return invoke<TResponse>('dq_list_models', {
          provider_id: typeof providerId === 'number' ? providerId : undefined,
        });
      }
      case route === 'GET /health': {
        const providerId = options.query?.provider_id;
        return invoke<TResponse>('dq_health_check', {
          provider_id: typeof providerId === 'number' ? providerId : undefined,
        });
      }
      case route === 'GET /chats': {
        return invoke<TResponse>('dq_list_chats');
      }
      case /^GET \/chats\/\d+\/messages$/.test(route): {
        const id = Number(options.path.split('/')[2]);
        return invoke<TResponse>('dq_get_chat_messages', { chat_id: id });
      }
      case /^DELETE \/chats\/\d+$/.test(route): {
        const id = Number(options.path.split('/')[2]);
        return invoke<TResponse>('dq_delete_chat', { chat_id: id });
      }
      case /^PUT \/chats\/\d+$/.test(route): {
        const id = Number(options.path.split('/')[2]);
        const body = (options.body ?? {}) as { title?: string };
        return invoke<TResponse>('dq_rename_chat', {
          chat_id: id,
          title: body.title ?? '',
        });
      }
      case /^POST \/chats\/\d+\/branch$/.test(route): {
        const id = Number(options.path.split('/')[2]);
        return invoke<TResponse>('dq_branch_chat', { chat_id: id, payload: options.body });
      }
      case route === 'POST /health/preview': {
        return invoke<TResponse>('dq_health_check_preview', { payload: options.body });
      }
      default:
        throw new Error(`Unsupported Tauri route: ${route}`);
    }
  }

  stream(options: TransportStreamOptions): TransportStreamHandle {
    // 生成本次流的唯一标识
    const streamId = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
    const queue: Types.StreamEvent[] = [];
    let ended = false;
    let startedResolve: () => void = () => {};
    const started = new Promise<void>((resolve) => {
      startedResolve = () => {
        resolve();
        startedResolve = () => {};
      };
    });

    const invokePromise = ensureInvoke();
    const eventApiPromise = ensureEventApi();
    let unlisteners: Array<() => void> = [];

    const tryEnqueue = (payload: any, mapper: (p: any) => Types.StreamEvent | null) => {
      if (!payload || payload.stream_id !== streamId) return;
      const ev = mapper(payload.data);
      if (ev) queue.push(ev);
    };

    const events = (async function* (): AsyncGenerator<Types.StreamEvent> {
      const invoke = await invokePromise;
      const { listen } = await eventApiPromise;

      try {
        unlisteners.push(
          await listen('dq:meta', (ev: any) =>
            tryEnqueue(ev.payload, (d) => ({ type: 'meta', chatId: d.chat_id })),
          ),
        );
        unlisteners.push(
          await listen('dq:log', (ev: any) =>
            tryEnqueue(ev.payload, (d) => ({ type: 'log', level: 'log', message: String(d) })),
          ),
        );
        unlisteners.push(
          await listen('dq:chunk', (ev: any) =>
            tryEnqueue(ev.payload, (d) => ({ type: 'chunk', text: String(d) })),
          ),
        );
        unlisteners.push(
          await listen('dq:error', (ev: any) =>
            tryEnqueue(ev.payload, (d) => ({ type: 'error', message: String(d) })),
          ),
        );
        unlisteners.push(
          await listen('dq:end', (ev: any) => {
            if (ev?.payload?.stream_id === streamId) {
              ended = true;
            }
          }),
        );

        await invoke('dq_send_chat_stream', {
          stream_id: streamId,
          prompt: options.prompt,
          chat_id: options.chatId,
          provider_id: options.providerId,
          regen_message_id: options.regenMessageId,
          stream: options.stream,
          debug: options.debug,
        });

        startedResolve();

        while (!ended || queue.length > 0) {
          if (queue.length === 0) {
            await new Promise((r) => setTimeout(r, 15));
            continue;
          }
          const ev = queue.shift()!;
          yield ev;
        }
      } finally {
        ended = true;
        for (const unlisten of unlisteners) {
          try {
            unlisten();
          } catch {
            // 忽略释放异常
          }
        }
        unlisteners = [];
        startedResolve();
      }
    })();

    const cancel = () => {
      ended = true;
      void (async () => {
        try {
          const invoke = await invokePromise;
          await started.catch(() => undefined);
          await invoke('dq_cancel_stream', { stream_id: streamId });
        } catch {
          // 若取消失败，不阻塞 UI。
        }
      })();
    };

    return { events, cancel };
  }
}
