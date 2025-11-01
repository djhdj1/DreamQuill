import type {
  HealthStatus,
  ProviderConfig,
  ProviderRecord,
  ProviderState,
} from '../types';
import type { Transport } from '../transport';

interface SaveOptions {
  /** @brief 保存后是否设为默认。 */
  setDefault?: boolean;
  /** @brief 可选遥测开关。 */
  telemetryEnabled?: boolean;
}

/** @brief Provider 服务封装。 */
export class ProviderService {
  constructor(private readonly transport: Transport) {}

  /** @brief 获取当前 Provider 状态。 */
  async fetchState(): Promise<ProviderState> {
    const raw = await this.transport.request<ProviderStateResponse>({
      method: 'GET',
      path: '/providers',
    });
    return normalizeState(raw);
  }

  /** @brief 新增 Provider。 */
  async create(config: ProviderConfig, options?: SaveOptions): Promise<ProviderState> {
    const raw = await this.transport.request<ProviderStateResponse>({
      method: 'POST',
      path: '/providers',
      body: toRequestPayload(config, options),
    });
    return normalizeState(raw);
  }

  /** @brief 更新 Provider。 */
  async update(id: number, config: ProviderConfig, options?: SaveOptions): Promise<ProviderState> {
    const raw = await this.transport.request<ProviderStateResponse>({
      method: 'PUT',
      path: `/providers/${id}`,
      body: toRequestPayload(config, options),
    });
    return normalizeState(raw);
  }

  /** @brief 删除 Provider。 */
  async remove(id: number): Promise<ProviderState> {
    const raw = await this.transport.request<ProviderStateResponse>({
      method: 'DELETE',
      path: `/providers/${id}`,
    });
    return normalizeState(raw);
  }

  /** @brief 设定默认 Provider。 */
  async selectDefault(id: number): Promise<ProviderState> {
    const raw = await this.transport.request<ProviderStateResponse>({
      method: 'POST',
      path: `/providers/${id}/select`,
    });
    return normalizeState(raw);
  }

  /** @brief 列出模型名称。 */
  async listModels(providerId?: number): Promise<string[]> {
    const response = await this.transport.request<{ models: string[] } | string[]>({
      method: 'GET',
      path: '/models',
      query: providerId !== undefined ? { provider_id: providerId } : undefined,
    });
    if (Array.isArray(response)) {
      return response;
    }
    return response.models;
  }

  /** @brief 健康检查：尝试列出模型并返回结果。 */
  async healthCheck(providerId?: number): Promise<HealthStatus> {
    const raw = await this.transport.request<any>({
      method: 'GET',
      path: '/health',
      query: providerId !== undefined ? { provider_id: providerId } : undefined,
    });
    return normalizeHealth(raw);
  }

  /**
   * @brief 健康检查预检：使用暂存的 Provider 配置进行测试。
   * @param config 当前表单内的 Provider 配置。
   */
  async healthCheckPreview(config: ProviderConfig): Promise<HealthStatus> {
    const raw = await this.transport.request<any>({
      method: 'POST',
      path: '/health/preview',
      body: toPreviewPayload(config),
    });
    return normalizeHealth(raw);
  }
}

function toRequestPayload(config: ProviderConfig, options?: SaveOptions) {
  return {
    name: config.name,
    provider: config.provider,
    api_base: config.apiBase,
    api_key: config.apiKey,
    model: config.model,
    set_default: options?.setDefault ?? false,
    telemetry_enabled: options?.telemetryEnabled,
  };
}

function toPreviewPayload(config: ProviderConfig) {
  return {
    name: config.name,
    provider: config.provider,
    api_base: config.apiBase,
    api_key: config.apiKey,
    model: config.model,
  };
}

function normalizeHealth(raw: any): HealthStatus {
  return {
    ok: Boolean(raw?.ok),
    providerId: Number(raw?.provider_id ?? 0),
    provider: typeof raw?.provider === 'string' ? raw.provider : undefined,
    base: typeof raw?.base === 'string' ? raw.base : undefined,
    model: typeof raw?.model === 'string' ? raw.model : undefined,
    models: typeof raw?.models === 'number' ? raw.models : undefined,
    error: typeof raw?.error === 'string' ? raw.error : undefined,
  };
}

interface ProviderRecordResponse {
  id: number;
  name?: string;
  provider?: string;
  api_base?: string;
  api_key?: string;
  model?: string;
  is_default?: boolean;
}

interface ProviderStateResponse {
  providers?: ProviderRecordResponse[];
  default_provider_id?: number | null;
  telemetry_enabled?: boolean;
}

function normalizeRecord(raw: ProviderRecordResponse): ProviderRecord {
  return {
    id: raw.id,
    name: raw.name ?? '',
    provider: raw.provider ?? 'openai',
    apiBase: raw.api_base ?? '',
    apiKey: raw.api_key ?? '',
    model: raw.model ?? '',
    isDefault: Boolean(raw.is_default),
  };
}

function normalizeState(raw: ProviderStateResponse): ProviderState {
  return {
    providers: (raw.providers ?? []).map(normalizeRecord),
    defaultProviderId:
      raw.default_provider_id === undefined || raw.default_provider_id === null
        ? null
        : Number(raw.default_provider_id),
    telemetryEnabled: Boolean(raw.telemetry_enabled),
  };
}
