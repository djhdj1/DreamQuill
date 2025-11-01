import type * as Types from '../types';
import type {
  Transport,
  TransportRequestOptions,
  TransportStreamHandle,
  TransportStreamOptions,
} from '../transport';

/** @brief HTTP 运行时下的通用传输实现。 */
export class HttpTransport implements Transport {
  /** @brief 基础路径，默认为 /api。 */
  private readonly base: string;

  constructor(basePath: string = '/api') {
    this.base = basePath.replace(/\/$/, '');
  }

  /** @brief 拼装查询字符串。 */
  private buildUrl(path: string, query?: Record<string, string | number | boolean | undefined>): string {
    const url = new URL(`${this.base}${path}`, window.location.origin);
    if (query) {
      Object.entries(query).forEach(([key, value]) => {
        if (value === undefined) return;
        url.searchParams.set(key, String(value));
      });
    }
    return url.toString();
  }

  async request<TResponse>(options: TransportRequestOptions<TResponse>): Promise<TResponse> {
    const url = this.buildUrl(options.path, options.query);
    const resp = await fetch(url, {
      method: options.method,
      headers: { 'content-type': 'application/json' },
      body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
    });
    if (!resp.ok) {
      const text = await resp.text();
      throw new Error(`HTTP ${resp.status}: ${text}`);
    }
    const data = (await resp.json()) as unknown;
    return options.parse ? options.parse(data) : (data as TResponse);
  }

  stream(options: TransportStreamOptions): TransportStreamHandle {
    const url = this.buildUrl(options.path, {
      prompt: options.prompt,
      chat_id: options.chatId,
      provider_id: options.providerId,
      regen_message_id: options.regenMessageId,
      stream: options.stream === false ? 'false' : undefined,
      debug: options.debug ? 'true' : undefined,
    });

    let stopped = false;
    let es: EventSource | null = null;

    const events = (async function* (): AsyncGenerator<Types.StreamEvent> {
      const queue: Types.StreamEvent[] = [];
      const listeners = new Map<string, (ev: MessageEvent) => void>();

      const enqueue = (event: Types.StreamEvent) => {
        queue.push(event);
      };

      const flush = async function* (): AsyncGenerator<Types.StreamEvent, void, unknown> {
        while (queue.length > 0) {
          yield queue.shift() as Types.StreamEvent;
        }
      };

      es = new EventSource(url);
      try {
        if (stopped) {
          return;
        }

        listeners.set('meta', (ev) => {
          try {
            const payload = JSON.parse(ev.data || '{}');
            if (typeof payload.chat_id === 'number') {
              enqueue({ type: 'meta', chatId: payload.chat_id });
            }
          } catch (error) {
            enqueue({ type: 'log', level: 'error', message: `meta parse error: ${String(error)}` });
          }
        });

        listeners.set('log', (ev) => {
          enqueue({ type: 'log', level: 'log', message: ev.data || '' });
        });

        listeners.set('error', (ev) => {
          const data = (ev as MessageEvent).data as string | undefined;
          if (data) {
            enqueue({ type: 'error', message: data });
          } else {
            enqueue({ type: 'log', level: 'info', message: 'SSE closed' });
          }
          stopped = true;
          es?.close();
        });

        listeners.forEach((handler, name) => es?.addEventListener(name, handler as EventListener));
        es.onmessage = (ev) => {
          enqueue({ type: 'chunk', text: ev.data ?? '' });
        };

        while (!stopped) {
          if (queue.length === 0) {
            await new Promise((resolve) => setTimeout(resolve, 20));
          }
          yield* flush();
          if (!es || es.readyState === EventSource.CLOSED) {
            stopped = true;
            break;
          }
        }

        // 清空残留队列
        yield* flush();
      } finally {
        listeners.forEach((handler, name) => es?.removeEventListener(name, handler as EventListener));
        es?.close();
        es = null;
        stopped = true;
      }
    })();

    const cancel = () => {
      if (stopped) {
        return;
      }
      stopped = true;
      if (es) {
        es.close();
      }
    };

    return { events, cancel };
  }
}
