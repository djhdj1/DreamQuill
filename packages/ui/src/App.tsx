import { useEffect, useRef, useState } from 'react';
import {
  createProvider,
  deleteProvider,
  deleteChat as deleteChatApi,
  fetchChatMessages,
  fetchProviderState,
  listChats,
  listModels,
  selectProvider,
  healthCheck,
  healthCheckPreview,
  sendChat,
  branchChat,
  updateProvider,
  renameChat,
  type ChatStreamHandle,
} from './api';
import type {
  ChatSummary,
  DebugLog,
  ProviderConfig,
  ProviderRecord,
  SendChatParams,
  StoredChatMessage,
} from './types';

type UiMessage = {
  id: number | null;
  role: StoredChatMessage['role'];
  content: string;
  pending?: boolean;
};

const PROVIDER_TYPES = ['openai', 'openai-response', 'claude', 'gemini'];

const EMPTY_PROVIDER: ProviderConfig = {
  name: '未命名模型服务',
  provider: 'openai',
  apiBase: 'https://api.openai.com',
  apiKey: '',
  model: '',
};

async function copyToClipboard(text: string): Promise<void> {
  if (!text) {
    return;
  }
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const mod = await import('@tauri-apps/api/clipboard');
  await mod.writeText(text);
}

function App() {
  const [providers, setProviders] = useState<ProviderRecord[]>([]);
  const [providerForm, setProviderForm] = useState<ProviderConfig>({ ...EMPTY_PROVIDER });
  const [selectedProviderId, setSelectedProviderId] = useState<number | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [makeDefault, setMakeDefault] = useState(false);
  const [telemetryEnabled, setTelemetryEnabled] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const modelsRequestRef = useRef<number | null>(null);

  const [chatSummaries, setChatSummaries] = useState<ChatSummary[]>([]);
  const [activeChatId, setActiveChatId] = useState<number | null>(null);
  const [selectedHistoryChatId, setSelectedHistoryChatId] = useState<number | null>(null);
  const [chatId, setChatId] = useState<number | undefined>(undefined);
  const [messages, setMessages] = useState<UiMessage[]>([]);

  const [prompt, setPrompt] = useState('');
  const [hint, setHint] = useState('');
  const [logs, setLogs] = useState<DebugLog[]>([]);
  const [debug, setDebug] = useState(false);
  const [stream, setStream] = useState(true);
  const [isBusy, setIsBusy] = useState(false);
  const [streamHandle, setStreamHandle] = useState<ChatStreamHandle | null>(null);

  const assistantIndexRef = useRef<number>(-1);
  const pendingChatIdRef = useRef<number | null>(null);

  const pushLog = (log: DebugLog) => setLogs((prev) => [...prev, log]);

  const providerLabel = (providerId: number | null) => {
    if (providerId == null) {
      return '未绑定';
    }
    const record = providers.find((item) => item.id === providerId);
    return record ? record.name : `服务 #${providerId}`;
  };

  const loadModelsForProvider = async (providerId: number, silent = false): Promise<void> => {
    modelsRequestRef.current = providerId;
    try {
      const list = await listModels(providerId);
      if (modelsRequestRef.current !== providerId) {
        return;
      }
      setModels(list);
      if (!silent) {
        setHint(`已获取 ${list.length} 个模型`);
      }
    } catch (error) {
      if (modelsRequestRef.current !== providerId) {
        return;
      }
      pushLog({ level: 'error', text: `刷新模型失败：${String(error)}` });
      if (!silent) {
        setHint('刷新模型失败，请查看日志');
      }
    }
  };

  const applySelection = (id: number, source?: ProviderRecord[]) => {
    const list = source ?? providers;
    const record = list.find((item) => item.id === id);
    if (!record) {
      return;
    }
    setSelectedProviderId(id);
    setEditingId(id);
    setProviderForm({
      name: record.name,
      provider: record.provider,
      apiBase: record.apiBase,
      apiKey: record.apiKey ?? '',
      model: record.model,
    });
    setMakeDefault(false);
    setHint('');
    void loadModelsForProvider(id, true);
  };

  const refreshState = async () => {
    try {
      const state = await fetchProviderState();
      setProviders(state.providers);
      setTelemetryEnabled(state.telemetryEnabled);
      if (state.providers.length === 0) {
        setSelectedProviderId(null);
        setEditingId(null);
        setProviderForm({ ...EMPTY_PROVIDER });
        setModels([]);
        modelsRequestRef.current = null;
        return;
      }
      const defaultId = state.defaultProviderId ?? state.providers[0].id;
      applySelection(defaultId, state.providers);
    } catch (error) {
      pushLog({ level: 'error', text: `加载模型服务失败：${String(error)}` });
    }
  };

  const loadChat = async (id: number, options?: { silent?: boolean }) => {
    try {
      const payload = await fetchChatMessages(id);
      const normalized = (payload.messages ?? []).map((msg) => ({
        id: msg.id,
        role: msg.role,
        content: msg.content,
      }));
      setActiveChatId(id);
      setChatId(id);
      setMessages(normalized);
      assistantIndexRef.current = -1;
      setSelectedHistoryChatId(id);
      if (!options?.silent) {
        setHint(`已载入会话 #${id}`);
      }
    } catch (error) {
      pushLog({ level: 'error', text: `载入会话失败：${String(error)}` });
    }
  };

  const refreshChats = async (preferredChatId?: number | null, forceReload = false) => {
    try {
      const list = await listChats();
      setChatSummaries(list);
      if (!list.length) {
        setActiveChatId(null);
        setChatId(undefined);
        setMessages([]);
        assistantIndexRef.current = -1;
        setSelectedHistoryChatId(null);
        return;
      }
      const desiredId = preferredChatId ?? activeChatId ?? list[0].id;
      const finalId = list.some((chat) => chat.id === desiredId) ? desiredId : list[0].id;
      if (forceReload || finalId !== activeChatId) {
        await loadChat(finalId, { silent: true });
      } else {
        setSelectedHistoryChatId(finalId);
      }
    } catch (error) {
      pushLog({ level: 'error', text: `加载历史会话失败：${String(error)}` });
    }
  };

  useEffect(() => {
    void (async () => {
      await refreshState();
      await refreshChats();
    })();
  }, []);

  const handleSave = async () => {
    const payload = { ...providerForm };
    try {
      const state =
        editingId == null
          ? await createProvider(payload, { setDefault: makeDefault, telemetryEnabled })
          : await updateProvider(editingId, payload, { setDefault: makeDefault, telemetryEnabled });
      setProviders(state.providers);
      setTelemetryEnabled(state.telemetryEnabled);
      const defaultId = state.defaultProviderId ?? state.providers[0]?.id ?? null;
      if (defaultId !== null) {
        applySelection(defaultId, state.providers);
      }
      setHint('模型服务保存成功');
      setMakeDefault(false);
    } catch (error) {
      pushLog({ level: 'error', text: `保存模型服务失败：${String(error)}` });
    }
  };

  const handleRefreshModels = async () => {
    const targetId = editingId ?? selectedProviderId;
    if (targetId == null) {
      pushLog({ level: 'error', text: '请先选择已保存的模型服务' });
      setHint('请先选择已保存的模型服务');
      return;
    }
    await loadModelsForProvider(targetId, false);
  };

  const handleHealthCheck = async () => {
    try {
      const targetId = editingId ?? selectedProviderId;
      const result =
        targetId != null
          ? await healthCheck(targetId)
          : await (async () => {
              if (!providerForm.apiBase.trim() || !providerForm.apiKey.trim()) {
                const message = '请填写 API Base 与 API Key 后再执行健康检查';
                pushLog({ level: 'error', text: message });
                setHint(message);
                return null;
              }
              return healthCheckPreview(providerForm);
            })();
      if (!result) {
        return;
      }
      if (result.ok) {
        setHint(`连接正常（models=${result.models ?? 0}）`);
        pushLog({
          level: 'info',
          text: `健康检查通过：provider=${result.provider} base=${result.base}`,
        });
      } else {
        setHint('连接异常');
        pushLog({ level: 'error', text: `健康检查失败：${result.error ?? '未知错误'}` });
      }
    } catch (error) {
      pushLog({ level: 'error', text: `健康检查调用失败：${String(error)}` });
    }
  };

  const handleSelectProvider = (id: number) => {
    applySelection(id);
  };

  const handleAddProvider = () => {
    setSelectedProviderId(null);
    setEditingId(null);
    setProviderForm({ ...EMPTY_PROVIDER, name: `模型服务-${providers.length + 1}` });
    setModels([]);
    modelsRequestRef.current = null;
    setMakeDefault(false);
    setHint('');
  };

  const handleDeleteProvider = async () => {
    if (editingId == null) {
      pushLog({ level: 'error', text: '当前没有可删除的模型服务' });
      return;
    }
    try {
      const state = await deleteProvider(editingId);
      setProviders(state.providers);
      setTelemetryEnabled(state.telemetryEnabled);
      if (state.providers.length === 0) {
        setSelectedProviderId(null);
        setEditingId(null);
        setProviderForm({ ...EMPTY_PROVIDER });
        setModels([]);
        modelsRequestRef.current = null;
      } else {
        const defaultId = state.defaultProviderId ?? state.providers[0].id;
        applySelection(defaultId, state.providers);
      }
      setMakeDefault(false);
      setHint('模型服务已删除');
    } catch (error) {
      pushLog({ level: 'error', text: `删除模型服务失败：${String(error)}` });
    }
  };

  const handleSetDefault = async () => {
    if (editingId == null) {
      pushLog({ level: 'error', text: '请先选择模型服务' });
      return;
    }
    try {
      const state = await selectProvider(editingId);
      setProviders(state.providers);
      setTelemetryEnabled(state.telemetryEnabled);
      const defaultId = state.defaultProviderId ?? editingId;
      applySelection(defaultId, state.providers);
      setMakeDefault(false);
    } catch (error) {
      pushLog({ level: 'error', text: `设置默认模型服务失败：${String(error)}` });
    }
  };

  const handleSelectChat = async (chatIdToLoad: number) => {
    pendingChatIdRef.current = chatIdToLoad;
    await loadChat(chatIdToLoad);
    pendingChatIdRef.current = null;
  };

  const handleCopyChat = async () => {
    if (messages.length === 0) {
      setHint('当前无聊天内容可复制');
      return;
    }
    const text = messages
      .map((msg) => `[${msg.role}] ${msg.content}`)
      .join('\n\n')
      .trim();
    try {
      await copyToClipboard(text);
      setHint('聊天内容已复制');
    } catch (error) {
      pushLog({ level: 'error', text: `复制聊天失败：${String(error)}` });
    }
  };

  const handleDeleteChat = async (chatIdToDelete?: number) => {
    const target = chatIdToDelete ?? activeChatId;
    if (target == null) {
      pushLog({ level: 'error', text: '当前没有可删除的会话' });
      return;
    }
    try {
      const list = await deleteChatApi(target);
      setChatSummaries(list);
      if (!list.length) {
      setActiveChatId(null);
      setChatId(undefined);
      setMessages([]);
      assistantIndexRef.current = -1;
      setSelectedHistoryChatId(null);
      setHint('会话已删除');
      return;
    }
      const fallbackId = list[0].id;
      const preferred = list.some((chat) => chat.id === activeChatId) ? activeChatId : fallbackId;
      if (preferred != null) {
        await loadChat(preferred ?? fallbackId, { silent: true });
      }
      setHint('会话已删除');
    } catch (error) {
      pushLog({ level: 'error', text: `删除会话失败：${String(error)}` });
    }
  };

  const resolveChatTarget = () => selectedHistoryChatId ?? activeChatId;

  const handleLoadSelectedChat = async () => {
    const target = resolveChatTarget();
    if (target == null) {
      pushLog({ level: 'error', text: '暂无可载入的会话' });
      setHint('暂无可载入的会话');
      return;
    }
    await handleSelectChat(target);
    setHint(`已载入会话 #${target}`);
  };

  const handleRenameChat = async () => {
    const target = resolveChatTarget();
    if (target == null) {
      pushLog({ level: 'error', text: '暂无可重命名的会话' });
      setHint('暂无可重命名的会话');
      return;
    }
    const current = chatSummaries.find((item) => item.id === target);
    const suggested = current?.title ?? `Chat ${target}`;
    const input = window.prompt('请输入新的会话标题', suggested);
    if (input === null) {
      return;
    }
    const trimmed = input.trim();
    if (!trimmed) {
      pushLog({ level: 'error', text: '会话标题不能为空' });
      setHint('会话标题不能为空');
      return;
    }
    try {
      const summary = await renameChat(target, trimmed);
      setChatSummaries((prev) =>
        prev.map((item) => (item.id === target ? { ...item, title: summary.title } : item)),
      );
      if (activeChatId === target) {
        setHint(`会话已重命名为「${summary.title}」`);
      } else {
        setHint('会话标题已更新');
      }
    } catch (error) {
      pushLog({ level: 'error', text: `重命名会话失败：${String(error)}` });
      setHint('重命名失败，请查看日志');
    }
  };

  const handleDuplicateChat = async () => {
    const target = resolveChatTarget();
    if (target == null) {
      pushLog({ level: 'error', text: '暂无可复制的会话' });
      setHint('暂无可复制的会话');
      return;
    }
    const baseTitle = chatSummaries.find((item) => item.id === target)?.title ?? `Chat ${target}`;
    const suggested = `${baseTitle} 副本`;
    const input = window.prompt('请输入新会话标题（可留空自动生成）', suggested);
    if (input === null) {
      return;
    }
    const trimmed = input.trim();
    try {
      const result = await branchChat(target, {
        title: trimmed ? trimmed : undefined,
      });
      setHint(`已复制会话为 #${result.chatId}`);
      await refreshChats(result.chatId, true);
    } catch (error) {
      pushLog({ level: 'error', text: `复制会话失败：${String(error)}` });
      setHint('复制会话失败，请查看日志');
    }
  };

  const selectedChatIdForActions = resolveChatTarget();

  const handleNewChat = () => {
    if (streamHandle) {
      streamHandle.cancel();
      setStreamHandle(null);
    }
    setActiveChatId(null);
    setChatId(undefined);
    setMessages([]);
    assistantIndexRef.current = -1;
    setSelectedHistoryChatId(null);
    setPrompt('');
    setHint('已开始新的会话');
  };

  const resolveProviderForNewChat = () => {
    if (selectedProviderId !== null) {
      return selectedProviderId;
    }
    const defaultProvider = providers.find((item) => item.isDefault);
    return defaultProvider?.id ?? null;
  };

  const runChatStream = async (
    params: SendChatParams,
    prepareMessages: () => { rollback: () => void } | null,
  ) => {
    if (streamHandle) {
      const message = '请先等待当前回复完成或手动中止。';
      pushLog({ level: 'info', text: message });
      setHint(message);
      return;
    }

    setIsBusy(true);
    const prepared = prepareMessages();
    if (!prepared) {
      setIsBusy(false);
      return;
    }
    const { rollback } = prepared;

    pendingChatIdRef.current = null;
    let currentHandle: ChatStreamHandle | null = null;
    let encounteredError = false;

    try {
      currentHandle = await sendChat(
        params,
        {
          onMeta: ({ chatId: id }) => {
            pendingChatIdRef.current = id;
            setActiveChatId(id);
            setChatId(id);
          },
          onChunk: (chunk) => {
            setMessages((prev) => {
              if (assistantIndexRef.current < 0 || assistantIndexRef.current >= prev.length) {
                return prev;
              }
              const next = [...prev];
              const current = next[assistantIndexRef.current];
              next[assistantIndexRef.current] = {
                ...current,
                pending: false,
                content: (current?.content ?? '') + chunk,
              };
              return next;
            });
          },
          onError: (msg) => {
            encounteredError = true;
            pushLog({ level: 'error', text: msg });
          },
          onLog: (log) => pushLog(log),
        },
      );
      setStreamHandle(currentHandle);
      await currentHandle.completed;
    } catch (error) {
      encounteredError = true;
      pushLog({ level: 'error', text: `发送失败：${String(error)}` });
    } finally {
      setStreamHandle((prev) => (prev === currentHandle ? null : prev));
      setIsBusy(false);
      const targetChatId = pendingChatIdRef.current ?? params.chatId ?? activeChatId;
      pendingChatIdRef.current = null;
      assistantIndexRef.current = -1;

      if (encounteredError) {
        rollback();
      }

      if (targetChatId != null) {
        await refreshChats(targetChatId, true);
      } else {
        await refreshChats(undefined, true);
      }
    }
  };

  const handleSend = async () => {
    const trimmed = prompt.trim();
    if (!trimmed) {
      return;
    }

    const providerForNewChat = activeChatId != null ? null : resolveProviderForNewChat();
    const effectiveProviderId =
      selectedProviderId ?? (activeChatId != null ? undefined : providerForNewChat ?? undefined);
    if (activeChatId == null && effectiveProviderId == null) {
      const message = '请先选择模型服务';
      pushLog({ level: 'error', text: message });
      setHint(message);
      return;
    }

    setPrompt('');
    if (debug) {
      pushLog({ level: 'info', text: '发送消息中…' });
    }

    await runChatStream(
      {
        chatId: activeChatId ?? undefined,
        providerId: effectiveProviderId ?? undefined,
        prompt: trimmed,
        stream,
        debug,
      },
      () => {
        let snapshot: UiMessage[] = [];
        setMessages((prev) => {
          snapshot = prev.map((item) => ({ ...item })) as UiMessage[];
          const next: UiMessage[] = [
            ...snapshot,
            { id: null, role: 'user', content: trimmed },
            { id: null, role: 'assistant', content: '', pending: true },
          ];
          assistantIndexRef.current = next.length - 1;
          return next;
        });
        return {
          rollback: () => setMessages(snapshot),
        };
      },
    );
  };

  const handleRegenerate = async (messageId: number) => {
    if (activeChatId == null) {
      const message = '当前无可重新生成的会话';
      pushLog({ level: 'error', text: message });
      setHint(message);
      return;
    }

    const targetIndex = messages.findIndex((msg) => msg.id === messageId && msg.role === 'assistant');
    if (targetIndex === -1) {
      pushLog({ level: 'error', text: '未找到待重新生成的回复' });
      return;
    }
    if (messages[targetIndex]?.pending) {
      pushLog({ level: 'info', text: '该回复仍在生成中，稍后再试。' });
      return;
    }

    setHint('正在重新生成回复…');
    await runChatStream(
      {
        chatId: activeChatId,
        providerId: selectedProviderId ?? undefined,
        prompt: '',
        stream,
        debug,
        regenMessageId: messageId,
      },
      () => {
        let snapshot: UiMessage[] = [];
        setMessages((prev) => {
          snapshot = prev.map((item) => ({ ...item })) as UiMessage[];
          const head = prev.slice(0, targetIndex).map((item) => ({ ...item })) as UiMessage[];
          const next: UiMessage[] = [
            ...head,
            { id: null, role: 'assistant', content: '', pending: true },
          ];
          assistantIndexRef.current = next.length - 1;
          return next;
        });
        return {
          rollback: () => setMessages(snapshot),
        };
      },
    );
  };

  const handleBranch = async (messageId: number) => {
    if (streamHandle || isBusy) {
      const text = '请先等待当前操作完成';
      pushLog({ level: 'info', text });
      setHint(text);
      return;
    }
    if (activeChatId == null) {
      const message = '当前没有可分支的会话';
      pushLog({ level: 'error', text: message });
      setHint(message);
      return;
    }
    const targetExists = messages.some((msg) => msg.id === messageId);
    if (!targetExists) {
      pushLog({ level: 'error', text: '未找到目标消息，无法分支' });
      return;
    }

    const currentTitle =
      chatSummaries.find((item) => item.id === activeChatId)?.title ?? `Chat ${activeChatId}`;
    const suggested = `${currentTitle} 分支`;
    const input = window.prompt('请输入新分支标题（可留空使用默认）', suggested);
    if (input === null) {
      return;
    }

    try {
      const result = await branchChat(activeChatId, {
        untilMessageId: messageId,
        title: input.trim() ? input.trim() : undefined,
      });
      setHint(`已创建分支会话 #${result.chatId}`);
      await refreshChats(result.chatId, true);
    } catch (error) {
      pushLog({ level: 'error', text: `创建分支失败：${String(error)}` });
    }
  };

  const handleCancel = () => {
    if (!streamHandle) {
      return;
    }
    streamHandle.cancel();
    setStreamHandle(null);
    setIsBusy(false);
    assistantIndexRef.current = -1;
    setHint('已请求中止当前回复');
  };

  return (
    <div>
      <header>
        <div>
          <strong>DreamQuill</strong> · Desktop Preview
        </div>
        <div className="muted">M1：多模型服务最小可聊</div>
      </header>
      <main>
        <aside>
          <div className="section-title">模型服务清单</div>
          <div className="form-field">
            <label>选择模型服务</label>
            <select
              value={selectedProviderId !== null ? String(selectedProviderId) : ''}
              onChange={(e) => {
                const value = e.target.value;
                if (!value) {
                  return;
                }
                handleSelectProvider(Number(value));
              }}
            >
              {!providers.length && <option value="">暂无模型服务</option>}
              {providers.map((item) => (
                <option key={item.id} value={String(item.id)}>
                  {item.name}
                  {item.isDefault ? ' (默认)' : ''}
                </option>
              ))}
            </select>
          </div>
          <div className="form-field" style={{ display: 'flex', gap: '8px' }}>
            <button type="button" onClick={handleAddProvider} className="secondary" style={{ flex: 1 }}>
              新建模型服务
            </button>
            <button
              type="button"
              onClick={handleDeleteProvider}
              className="secondary"
              style={{ flex: 1 }}
              disabled={editingId == null}
            >
              删除
            </button>
          </div>

          <div className="section-title">模型服务配置</div>
          <div className="form-field">
            <label>名称</label>
            <input
              value={providerForm.name}
              onChange={(e) => setProviderForm((prev) => ({ ...prev, name: e.target.value }))}
            />
          </div>
          <div className="form-field">
            <label>类型</label>
            <select
              value={providerForm.provider}
              onChange={(e) => setProviderForm((prev) => ({ ...prev, provider: e.target.value }))}
            >
              {PROVIDER_TYPES.map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </select>
          </div>
          <div className="form-field">
            <label>API Base</label>
            <input
              value={providerForm.apiBase}
              onChange={(e) => setProviderForm((prev) => ({ ...prev, apiBase: e.target.value }))}
              placeholder="https://api.openai.com"
            />
          </div>
          <div className="form-field">
            <label>API Key</label>
            <input
              value={providerForm.apiKey}
              onChange={(e) => setProviderForm((prev) => ({ ...prev, apiKey: e.target.value }))}
              placeholder="sk-..."
            />
          </div>
          <div className="form-field">
            <label>模型</label>
            <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
              <select
                value={providerForm.model}
                onChange={(e) => setProviderForm((prev) => ({ ...prev, model: e.target.value }))}
                style={{ flex: 1 }}
              >
                <option value="">请选择或手填</option>
                {models.map((model) => (
                  <option key={model} value={model}>
                    {model}
                  </option>
                ))}
              </select>
              <button type="button" className="secondary" onClick={handleRefreshModels}>
                刷新
              </button>
              <button type="button" className="secondary" onClick={handleHealthCheck}>
                健康检查
              </button>
            </div>
          </div>
          <div className="form-field">
            <label className="muted">
              <input
                type="checkbox"
                checked={makeDefault}
                onChange={(e) => setMakeDefault(e.target.checked)}
              />
              保存时设为默认
            </label>
          </div>
          <div className="form-field">
            <button type="button" onClick={handleSave}>
              保存模型服务
            </button>
          </div>
          <div className="form-field" style={{ display: 'flex', gap: '8px' }}>
            <button
              type="button"
              onClick={handleSetDefault}
              className="secondary"
              disabled={editingId == null}
              style={{ flex: 1 }}
            >
              设为默认
            </button>
            <label className="muted" style={{ flex: 1 }}>
              <input
                type="checkbox"
                checked={telemetryEnabled}
                onChange={(e) => setTelemetryEnabled(e.target.checked)}
              />
              启用本地遥测
            </label>
          </div>

          <div className="section-title">历史会话</div>
          <div className="form-field">
            <label>选择会话</label>
            <select
              value={selectedHistoryChatId != null ? String(selectedHistoryChatId) : ''}
              onChange={(e) => {
                const value = e.target.value;
                if (!value) {
                  setSelectedHistoryChatId(null);
                  return;
                }
                const id = Number(value);
                if (Number.isNaN(id)) {
                  setSelectedHistoryChatId(null);
                  return;
                }
                setSelectedHistoryChatId(id);
              }}
              disabled={!chatSummaries.length}
            >
              {chatSummaries.length === 0 ? (
                <option value="">暂无历史会话</option>
              ) : (
                chatSummaries.map((chat) => (
                  <option key={chat.id} value={String(chat.id)}>
                    {chat.title} · {providerLabel(chat.providerId)}
                    {activeChatId === chat.id ? ' (当前)' : ''}
                  </option>
                ))
              )}
            </select>
          </div>
          <div className="form-field" style={{ display: 'flex', gap: '8px' }}>
            <button
              type="button"
              className="secondary"
              style={{ flex: 1 }}
              onClick={handleLoadSelectedChat}
              disabled={selectedChatIdForActions == null}
            >
              载入会话
            </button>
            <button
              type="button"
              className="secondary"
              style={{ flex: 1 }}
              onClick={handleDuplicateChat}
              disabled={selectedChatIdForActions == null}
            >
              复制会话
            </button>
          </div>
          <div className="form-field" style={{ display: 'flex', gap: '8px' }}>
            <button
              type="button"
              className="secondary"
              style={{ flex: 1 }}
              onClick={handleRenameChat}
              disabled={selectedChatIdForActions == null}
            >
              重命名
            </button>
            <button
              type="button"
              className="secondary"
              style={{ flex: 1 }}
              onClick={() => handleDeleteChat(selectedChatIdForActions ?? undefined)}
              disabled={selectedChatIdForActions == null}
            >
              删除
            </button>
          </div>
        </aside>
        <section className="content">
          {hint && (
            <div className="muted" style={{ padding: '8px 12px' }}>
              {hint}
            </div>
          )}
          <div className="chat-box">
            {messages.length === 0 ? (
              <div className="chat-empty">暂无聊天内容，发送消息开始新会话。</div>
            ) : (
              messages.map((msg, idx) => (
                <div key={`${msg.id ?? 'pending'}-${idx}`} className={`msg ${msg.role}`}>
                  <div className="msg-body">
                    {msg.content}
                    {msg.pending && <span className="muted">（生成中…）</span>}
                  </div>
                  {msg.role === 'assistant' && msg.id != null && (
                    <div className="msg-actions">
                      <button
                        type="button"
                        className="secondary"
                        onClick={() => handleRegenerate(msg.id as number)}
                        disabled={isBusy || !!streamHandle}
                      >
                        重新生成
                      </button>
                      <button
                        type="button"
                        className="secondary"
                        onClick={() => handleBranch(msg.id as number)}
                        disabled={isBusy || !!streamHandle}
                      >
                        分支
                      </button>
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
          <div className="form-field" style={{ padding: '0 12px' }}>
            <label className="muted">当前 Chat ID</label>
            <input value={chatId ?? ''} readOnly placeholder="自动创建" />
          </div>
          <div className="form-field" style={{ display: 'flex', gap: '8px', padding: '0 12px' }}>
            <button
              type="button"
              className="secondary"
              onClick={() => refreshChats(activeChatId, true)}
            >
              刷新聊天
            </button>
            <button
              type="button"
              className="secondary"
              onClick={handleCopyChat}
              disabled={!messages.length}
            >
              复制内容
            </button>
            <button type="button" className="secondary" onClick={handleNewChat}>
              新建会话
            </button>
          </div>
          <div className="input-row">
            <input
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  handleSend();
                }
              }}
              placeholder="输入消息…"
              style={{ flex: 1 }}
              disabled={isBusy || (!providers.length && activeChatId === null)}
            />
            <button
              type="button"
              onClick={handleSend}
              disabled={isBusy || (!providers.length && activeChatId === null)}
            >
              {isBusy ? '发送中…' : '发送'}
            </button>
            <button
              type="button"
              onClick={handleCancel}
              className="secondary"
              disabled={!streamHandle}
            >
              中止
            </button>
          </div>
          <div className="checkbox-row">
            <label className="muted">
              <input type="checkbox" checked={debug} onChange={(e) => setDebug(e.target.checked)} /> 调试
            </label>
            <label className="muted">
              <input type="checkbox" checked={!stream} onChange={(e) => setStream(!e.target.checked)} /> 非流式
            </label>
          </div>
          {logs.length > 0 && (
            <pre className="log-panel">
              {logs.map((log) => `${log.level.toUpperCase()}: ${log.text}`).join('\n')}
            </pre>
          )}
        </section>
      </main>
    </div>
  );
}

export default App;
