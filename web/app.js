/**
 * @brief 浏览器静态版模型服务管理与聊天调试台（多 Provider 版本）。
 */
(function () {
  const $ = (sel) => document.querySelector(sel);

  const providerSelect = $('#providerSelect');
  const addProviderBtn = $('#addProviderBtn');
  const deleteProviderBtn = $('#deleteProviderBtn');
  const setDefaultBtn = $('#setDefaultBtn');
  const saveBtn = $('#saveBtn');
  const refreshModelsBtn = $('#refreshModelsBtn');
  const providerNameInput = $('#providerName');
  const providerTypeSelect = $('#providerType');
  const apiBaseInput = $('#apiBase');
  const apiKeyInput = $('#apiKey');
  const modelSelect = $('#modelSel');
  const modelInput = $('#modelInput');
  const makeDefaultToggle = $('#makeDefaultToggle');
  const telemetryToggle = $('#telemetryToggle');
  const hint = $('#hint');
  const chatIdInput = $('#chatId');
  const currentChatIdInput = $('#currentChatId');
  const chatList = $('#chatList');
  const deleteChatBtn = $('#deleteChatBtn');
  const chatBox = $('#chatBox');
  const promptInput = $('#prompt');
  const sendBtn = $('#sendBtn');
  const cancelBtn = $('#cancelBtn');
  const debugToggle = $('#debugToggle');
  const streamToggle = $('#streamToggle');
  const newChatBtn = $('#newChatBtn');
  const clearLogBtn = $('#clearLogBtn');
  const logPanel = $('#logPanel');
  const setupBanner = $('#setupBanner');

  const EMPTY_PROVIDER = {
    id: null,
    name: '新建模型服务',
    provider: 'openai',
    apiBase: 'https://api.openai.com',
    apiKey: '',
    model: '',
    isDefault: false,
  };

  let providers = [];
  let editingProviderId = null;
  let selectedProviderId = null;
  let telemetryEnabled = false;
  let currentModels = [];
  let chats = [];
  let selectedChatId = null;
  let currentEventSource = null;

  const setHint = (text = '') => {
    hint.textContent = text;
  };

  const log = (text, level = 'info') => {
    logPanel.textContent += `[${level}] ${text}\n`;
    logPanel.scrollTop = logPanel.scrollHeight;
  };

  const updateProviderButtons = () => {
    const disabled = editingProviderId == null;
    deleteProviderBtn.disabled = disabled;
    setDefaultBtn.disabled = disabled;
    refreshModelsBtn.disabled = disabled;
  };

  const updateChatButtons = () => {
    deleteChatBtn.disabled = selectedChatId == null;
  };

  const appendMsg = (role, text) => {
    const div = document.createElement('div');
    div.className = `msg ${role}`;
    div.textContent = text;
    chatBox.appendChild(div);
    chatBox.scrollTop = chatBox.scrollHeight;
    return div;
  };

  const renderMessages = (messageList) => {
    chatBox.innerHTML = '';
    messageList.forEach((msg) => appendMsg(msg.role, msg.content));
  };

  const updateModelSelect = (models, selectedValue) => {
    currentModels = models;
    modelSelect.innerHTML = '';
    const placeholder = document.createElement('option');
    placeholder.value = '';
    placeholder.textContent = models.length ? '请选择模型' : '无可用模型';
    modelSelect.appendChild(placeholder);
    models.forEach((m) => {
      const opt = document.createElement('option');
      opt.value = m;
      opt.textContent = m;
      modelSelect.appendChild(opt);
    });
    if (selectedValue && models.includes(selectedValue)) {
      modelSelect.value = selectedValue;
    } else {
      modelSelect.value = '';
    }
  };

  const renderProviderList = (selectedId) => {
    providerSelect.innerHTML = '';
    if (!providers.length) {
      const opt = document.createElement('option');
      opt.value = '';
      opt.textContent = '暂无模型服务';
      providerSelect.appendChild(opt);
      providerSelect.disabled = true;
      setupBanner.style.display = 'block';
      return;
    }
    providerSelect.disabled = false;
    setupBanner.style.display = 'none';
    providers.forEach((item) => {
      const opt = document.createElement('option');
      opt.value = item.id;
      opt.textContent = `${item.name}${item.isDefault ? '（默认）' : ''}`;
      providerSelect.appendChild(opt);
    });
    if (selectedId != null && providers.some((item) => item.id === selectedId)) {
      providerSelect.value = String(selectedId);
    } else {
      providerSelect.value = String(providers[0].id);
    }
  };

  const renderChatList = () => {
    chatList.innerHTML = '';
    if (!chats.length) {
      const li = document.createElement('li');
      li.className = 'chat-empty';
      li.textContent = '暂无历史会话';
      chatList.appendChild(li);
      selectedChatId = null;
      updateChatButtons();
      return;
    }
    const getProviderName = (providerId) => {
      if (!providerId) return '未绑定';
      const item = providers.find((p) => p.id === providerId);
      return item ? item.name : `未知服务:${providerId}`;
    };
    chats.forEach((chat) => {
      const li = document.createElement('li');
      li.className = `chat-item${chat.id === selectedChatId ? ' active' : ''}`;
      li.dataset.id = String(chat.id);
      li.dataset.providerId = chat.providerId != null ? String(chat.providerId) : '';
      const name = chat.title || `会话 ${chat.id}`;
      li.innerHTML = `<span>${name}</span><span>${getProviderName(chat.providerId)}</span>`;
      chatList.appendChild(li);
    });
    updateChatButtons();
  };

  const normalizeProviderRecord = (raw) => ({
    id: raw.id,
    name: raw.name || `模型服务-${raw.id}`,
    provider: raw.provider,
    apiBase: raw.api_base,
    apiKey: raw.api_key,
    model: raw.model,
    isDefault: Boolean(raw.is_default),
  });

  const applyState = (data) => {
    providers = (data.providers || []).map(normalizeProviderRecord);
    telemetryEnabled = Boolean(data.telemetry_enabled);
    telemetryToggle.checked = telemetryEnabled;
    const preferId = editingProviderId && providers.some((item) => item.id === editingProviderId)
      ? editingProviderId
      : data.default_provider_id ?? (providers[0]?.id ?? null);
    renderProviderList(preferId);
    if (preferId != null) {
      applySelection(preferId);
    } else {
      selectedProviderId = null;
      editingProviderId = null;
      fillForm({ ...EMPTY_PROVIDER });
      updateModelSelect([], '');
      chats = [];
      selectedChatId = null;
      renderChatList();
      updateProviderButtons();
    }
  };

  const fillForm = (config) => {
    providerNameInput.value = config.name || '';
    providerTypeSelect.value = config.provider || 'openai';
    apiBaseInput.value = config.apiBase || '';
    apiKeyInput.value = config.apiKey || '';
    modelInput.value = config.model || '';
    updateModelSelect(currentModels, config.model || '');
    makeDefaultToggle.checked = false;
    setHint('');
    updateProviderButtons();
  };

  const loadChatList = async () => {
    try {
      const res = await fetch('/api/chats');
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      chats = (data?.chats ?? []).map((chat) => ({
        id: chat.id,
        title: chat.title,
        providerId: chat.provider_id ?? null,
      }));
      if (!chats.some((chat) => chat.id === selectedChatId)) {
        selectedChatId = null;
      }
      renderChatList();
    } catch (error) {
      log(`加载历史会话失败：${error}`, 'error');
    }
  };

  const applySelection = (id) => {
    const record = providers.find((item) => item.id === id);
    if (!record) return;
    selectedProviderId = id;
    editingProviderId = id;
    providerSelect.value = String(id);
    currentModels = [];
    fillForm({
      name: record.name,
      provider: record.provider,
      apiBase: record.apiBase,
      apiKey: record.apiKey,
      model: record.model,
    });
    updateModelSelect([], record.model || '');
    loadChatList();
    updateProviderButtons();
  };

  const fetchState = async () => {
    try {
      const res = await fetch('/api/providers');
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      applyState(data);
      setHint('');
      if (!providers.length) {
        await loadChatList();
      }
    } catch (error) {
      log(`加载模型服务失败：${error}`, 'error');
    }
    updateProviderButtons();
  };

  const collectForm = () => ({
    name: providerNameInput.value.trim() || '未命名模型服务',
    provider: providerTypeSelect.value,
    api_base: apiBaseInput.value.trim(),
    api_key: apiKeyInput.value.trim(),
    model: modelInput.value.trim() || modelSelect.value.trim(),
    telemetry_enabled: telemetryToggle.checked,
    set_default: makeDefaultToggle.checked,
  });

  const createProvider = async (body) => {
    const res = await fetch('/api/providers', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    applyState(data);
    const match = providers
      .slice()
      .reverse()
      .find((item) => item.name === body.name && item.apiBase === body.api_base);
    if (match) applySelection(match.id);
    await loadChatList();
  };

  const updateProvider = async (id, body) => {
    const res = await fetch(`/api/providers/${id}`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    applyState(data);
    applySelection(id);
    await loadChatList();
  };

  const deleteProvider = async (id) => {
    const res = await fetch(`/api/providers/${id}`, { method: 'DELETE' });
    if (!res.ok) throw new Error(await res.text() || `HTTP ${res.status}`);
    const data = await res.json();
    applyState(data);
    await loadChatList();
  };

  const selectDefaultProvider = async (id) => {
    const res = await fetch(`/api/providers/${id}/select`, { method: 'POST' });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    applyState(data);
    await loadChatList();
  };

  const fetchModels = async () => {
    if (editingProviderId == null) {
      log('请先保存模型服务后再刷新模型列表', 'error');
      return;
    }
    try {
      const res = await fetch(`/api/models?provider_id=${editingProviderId}`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      const models = data?.models ?? [];
      updateModelSelect(models, modelInput.value.trim());
      setHint(`已获取 ${models.length} 个模型`);
    } catch (error) {
      log(`刷新模型失败：${error}`, 'error');
    }
  };

  const loadChatMessages = async (chatId) => {
    try {
      const res = await fetch(`/api/chats/${chatId}/messages`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      selectedChatId = chatId;
      renderChatList();
      renderMessages(data.messages || []);
      chatIdInput.value = data.chat_id;
      currentChatIdInput.value = data.chat_id;
      if (data.provider_id && data.provider_id !== editingProviderId) {
        const record = providers.find((item) => item.id === data.provider_id);
        if (record) {
          selectedProviderId = record.id;
          editingProviderId = record.id;
          providerSelect.value = String(record.id);
          fillForm({
            name: record.name,
            provider: record.provider,
            apiBase: record.apiBase,
            apiKey: record.apiKey,
            model: record.model,
          });
          updateModelSelect([], record.model || '');
        }
      }
      updateChatButtons();
    } catch (error) {
      log(`加载会话失败：${error}`, 'error');
    }
  };

  const handleSave = async () => {
    const body = collectForm();
    if (!body.api_base) {
      setHint('请填写 API 基地址');
      return;
    }
    try {
      if (editingProviderId == null) {
        await createProvider(body);
      } else {
        await updateProvider(editingProviderId, body);
      }
      makeDefaultToggle.checked = false;
      setHint('保存成功');
    } catch (error) {
      log(`保存失败：${error}`, 'error');
      setHint('保存失败，请查看日志');
    }
  };

  const handleDeleteProvider = async () => {
    if (editingProviderId == null) {
      log('当前无可删除的模型服务', 'error');
      return;
    }
    if (!confirm('确定删除该模型服务？若存在相关会话请先删除会话。')) return;
    try {
      await deleteProvider(editingProviderId);
      log('模型服务已删除');
      setHint('模型服务已删除');
    } catch (error) {
      log(`删除失败：${error}`, 'error');
      setHint(String(error));
    }
  };

  const handleSetDefault = async () => {
    if (editingProviderId == null) {
      log('请先选择模型服务', 'error');
      return;
    }
    try {
      await selectDefaultProvider(editingProviderId);
      log('已设为默认模型服务');
      makeDefaultToggle.checked = false;
    } catch (error) {
      log(`设置默认失败：${error}`, 'error');
    }
  };

  const resetChat = () => {
    if (currentEventSource) {
      currentEventSource.close();
      currentEventSource = null;
      cancelBtn.disabled = true;
      log('当前回复已被中止。', 'info');
    }
    chatIdInput.value = '';
    currentChatIdInput.value = '';
    selectedChatId = null;
    renderMessages([]);
    log('已开启新的会话', 'info');
    updateChatButtons();
  };

  const deleteChat = async () => {
    if (selectedChatId == null) {
      log('请选择要删除的会话', 'error');
      return;
    }
    if (!confirm('确定删除该历史会话吗？该操作不可恢复。')) return;
    try {
      const res = await fetch(`/api/chats/${selectedChatId}`, { method: 'DELETE' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      chats = (data?.chats ?? []).map((chat) => ({
        id: chat.id,
        title: chat.title,
        providerId: chat.provider_id,
      }));
      selectedChatId = null;
      renderChatList();
      renderMessages([]);
      chatIdInput.value = '';
      currentChatIdInput.value = '';
      log('历史会话已删除', 'info');
      updateChatButtons();
    } catch (error) {
      log(`删除会话失败：${error}`, 'error');
    }
  };

  const sendChat = () => {
    const text = promptInput.value.trim();
    if (!text) return;
    if (currentEventSource) {
      log('请先等待当前回复完成或点击“中止”。', 'info');
      return;
    }
    const providerId = editingProviderId ?? selectedProviderId;
    if (!providerId) {
      log('请先选择并保存模型服务', 'error');
      return;
    }

    const cid = chatIdInput.value.trim();
    const qs = new URLSearchParams();
    qs.set('prompt', text);
    if (cid) {
      qs.set('chat_id', cid);
    }
    qs.set('provider_id', providerId);
    if (streamToggle.checked) qs.set('stream', 'false');
    if (debugToggle.checked) qs.set('debug', 'true');

    appendMsg('user', text);
    const assistantEl = appendMsg('assistant', '');

    const es = new EventSource(`/api/chat/sse?${qs.toString()}`);
    currentEventSource = es;
    cancelBtn.disabled = false;
    es.onopen = () => log('[sse] open');
    es.addEventListener('meta', (ev) => {
      try {
        const data = JSON.parse(ev.data || '{}');
        if (data.chat_id) {
          chatIdInput.value = data.chat_id;
          currentChatIdInput.value = data.chat_id;
        }
      } catch (error) {
        log(`meta parse error: ${error}`, 'error');
      }
    });
    es.addEventListener('log', (ev) => log(ev.data || '[log event]', 'log'));
    es.addEventListener('error', (ev) => {
      const reason = ev?.data;
      if (reason) {
        log(reason, 'error');
        appendMsg('assistant', `[error] ${reason}`);
      } else {
        log('SSE 已关闭');
      }
      if (currentEventSource === es) {
        currentEventSource.close();
        currentEventSource = null;
        cancelBtn.disabled = true;
      } else {
        es.close();
      }
      const latestId = Number(chatIdInput.value || currentChatIdInput.value);
      if (!Number.isNaN(latestId)) {
        selectedChatId = latestId;
      }
      loadChatList();
    });
    es.onmessage = (ev) => {
      assistantEl.textContent += ev.data ?? '';
      chatBox.scrollTop = chatBox.scrollHeight;
    };

    promptInput.value = '';
    promptInput.focus();
  };

  providerSelect.addEventListener('change', (e) => {
    const value = Number(e.target.value);
    if (!Number.isNaN(value)) {
      applySelection(value);
    }
  });

  addProviderBtn.addEventListener('click', () => {
    selectedProviderId = null;
    editingProviderId = null;
    providerSelect.value = '';
    makeDefaultToggle.checked = false;
    currentModels = [];
    fillForm({ ...EMPTY_PROVIDER, name: `模型服务-${providers.length + 1}` });
    updateModelSelect([], '');
    chats = [];
    selectedChatId = null;
    renderChatList();
    updateProviderButtons();
  });

  deleteProviderBtn.addEventListener('click', handleDeleteProvider);
  setDefaultBtn.addEventListener('click', handleSetDefault);
  saveBtn.addEventListener('click', handleSave);
  refreshModelsBtn.addEventListener('click', fetchModels);
  newChatBtn.addEventListener('click', resetChat);
  clearLogBtn.addEventListener('click', () => {
    logPanel.textContent = '';
  });
  deleteChatBtn.addEventListener('click', deleteChat);

  telemetryToggle.addEventListener('change', (e) => {
    telemetryEnabled = e.target.checked;
    setHint('遥测开关将在保存后生效');
  });

  modelSelect.addEventListener('change', (e) => {
    if (e.target.value) {
      modelInput.value = e.target.value;
    }
  });
  modelInput.addEventListener('input', () => {
    if (modelInput.value) {
      modelSelect.value = '';
    }
  });

  chatList.addEventListener('click', (e) => {
    const item = e.target.closest('.chat-item');
    if (!item) return;
    const id = Number(item.dataset.id);
    if (Number.isNaN(id)) return;
    selectedChatId = id;
    renderChatList();
    loadChatMessages(id);
  });

  sendBtn.addEventListener('click', sendChat);
  promptInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendChat();
    }
  });

  cancelBtn.addEventListener('click', () => {
    if (!currentEventSource) {
      return;
    }
    currentEventSource.close();
    currentEventSource = null;
    cancelBtn.disabled = true;
    log('已请求中止当前回复。', 'info');
  });

  fetchState();
  updateProviderButtons();
  updateChatButtons();
})();
