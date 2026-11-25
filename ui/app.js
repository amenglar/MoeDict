/* MoeDict frontend — vanilla JS, no build step. */
(() => {
  'use strict';

  const TAURI = window.__TAURI__;
  if (!TAURI) {
    document.body.innerHTML =
      '<div class="fatal">未能连接到 Tauri 运行时。\n请使用 <code>cargo run -p moedict</code> 启动本应用。</div>';
    return;
  }

  const invoke = TAURI.core?.invoke ?? TAURI.invoke;
  const listen = TAURI.event?.listen;

  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => Array.from(root.querySelectorAll(selector));

  const DEFAULTS = Object.freeze({
    theme: 'system',
    fontSize: 16,
    autoIndex: true,
    suggestionLimit: 12,
    showAllDicts: false,
    defaultExpanded: 3,
  });
  const HISTORY_KEY = 'moedict.history.v1';
  const CONTENT_THEME_KEY = 'moedict.contentTheme.v1';
  const NAV_PANE_KEY = 'moedict.navPane.v1';
  const SIDEBAR_KEY = 'moedict.sidebar.v1';
  const TALL_THRESHOLD = 1500;

  const el = {
    dictList: $('#dict-list'),
    dictEmpty: $('#dict-empty'),
    dictFilter: $('#dict-filter'),
    dictTags: $('#dict-tags'),
    dictSummary: $('#dict-summary'),
    addBtn: $('#add-btn'),
    addPopover: $('#add-popover'),
    settingsBtn: $('#settings-btn'),
    searchShell: $('#search-shell'),
    historyBack: $('#history-back'),
    historyForward: $('#history-forward'),
    searchInput: $('#search-input'),
    clearBtn: $('#clear-btn'),
    navToggle: $('#nav-toggle'),
    navPane: $('#result-nav-pane'),
    navPaneClose: $('#nav-pane-close'),
    sidebarToggle: $('#sidebar-toggle'),
    sidebarReveal: $('#sidebar-reveal'),
    suggestions: $('#suggestions'),
    welcome: $('#welcome'),
    welcomeActions: $('#welcome-actions'),
    recent: $('#recent'),
    recentList: $('#recent-list'),
    clearHistoryBtn: $('#clear-history'),
    results: $('#results'),
    resultsList: $('#results-list'),
    resultsMeta: $('#results-meta'),
    resultNav: $('#result-nav'),
    noResults: $('#no-results'),
    searchLoading: $('#search-loading'),
    statusDicts: $('#status-dicts'),
    statusIndex: $('#status-index'),
    statusQuery: $('#status-query'),
    dropOverlay: $('#drop-overlay'),
    toasts: $('#toasts'),
    menuLayer: $('#menu-layer'),
    modalLayer: $('#modal-layer'),
  };

  const state = {
    dicts: [],
    settings: { ...DEFAULTS },
    contentTheme: 'alwaysLight',
    appInfo: null,
    resourceBase: 'mdict://localhost',
    query: '',
    hits: [],
    suggestions: [],
    suggestionIndex: -1,
    filter: '',
    tagFilter: '',
    indexing: new Map(),
    dragId: null,
    history: [],
    navStack: [],
    navIndex: -1,
    navigating: false,
    navPaneOpen: true,
    sidebarOpen: true,
    querySeq: 0,
    suggestSeq: 0,
    frameObservers: new Set(),
  };

  let suggestTimer = null;
  let refreshTimer = null;
  let cardObserver = null;
  let modalKeyHandler = null;
  let currentAudio = null;
  let focusSeq = 0;

  /* ============================== utilities ============================== */

  function icon(name) {
    return `<svg class="icon" aria-hidden="true"><use href="#i-${name}"/></svg>`;
  }

  function escapeHtml(value) {
    return String(value ?? '').replace(/[&<>"']/g, (char) => {
      switch (char) {
        case '&':
          return '&amp;';
        case '<':
          return '&lt;';
        case '>':
          return '&gt;';
        case '"':
          return '&quot;';
        default:
          return '&#39;';
      }
    });
  }

  function stripControls(value) {
    return String(value ?? '').replace(/[\u0000-\u001f\u007f]/g, '').trim();
  }

  function decodeSafe(value) {
    try {
      return decodeURIComponent(value);
    } catch {
      return value;
    }
  }

  function formatNumber(value) {
    const number = Number(value) || 0;
    try {
      return number.toLocaleString('zh-CN');
    } catch {
      return String(number);
    }
  }

  function formatDuration(ms) {
    if (!Number.isFinite(ms)) return '';
    return `${ms < 10 ? ms.toFixed(1) : Math.round(ms)} ms`;
  }

  function formatExpanded(value) {
    const count = Math.max(0, Number(value) || 0);
    return count === 0 ? '0 · 全部折叠' : `${count} 条`;
  }

  function resourceBase() {
    return state.resourceBase || 'mdict://localhost';
  }

  function resourceUrl(dictId, raw) {
    let value = stripControls(raw).replace(/\\/g, '/');
    if (!value) return '';
    if (/^(data:|blob:|#)/i.test(value)) return value;
    if (/^mdict:|^http:\/\/mdict\.localhost/i.test(value)) return value;
    if (/^(https?:)?\/\//i.test(value)) return value;
    value = value
      .replace(/^sound:\/\//i, '')
      .replace(/^file:\/\/\//i, '')
      .replace(/^\.\/+/, '')
      .replace(/^\/+/, '');
    if (!value) return '';
    const encoded = value
      .split('/')
      .map((part) => {
        let decoded = part;
        try {
          decoded = decodeURIComponent(part);
        } catch {
          /* keep as is */
        }
        return encodeURIComponent(decoded);
      })
      .join('/');
    return `${resourceBase()}/${dictId}/${encoded}`;
  }

  function rewriteCssUrls(css, dictId) {
    let value = String(css ?? '');
    value = value.replace(
      /url\(\s*(['"]?)([^'")]+)\1\s*\)/gi,
      (match, _quote, raw) => {
        const inner = stripControls(raw);
        if (!inner || /^(data:|blob:|#|https?:|\/\/)/i.test(inner)) return match;
        if (/^mdict:|^http:\/\/mdict\.localhost/i.test(inner)) return match;
        return `url("${resourceUrl(dictId, inner)}")`;
      }
    );
    value = value.replace(
      /@import\s+(['"])([^'"]+)\1/gi,
      (match, quote, raw) => {
        const inner = stripControls(raw);
        if (!inner || /^(data:|blob:|https?:|\/\/)/i.test(inner)) return match;
        return `@import ${quote}${resourceUrl(dictId, inner)}${quote}`;
      }
    );
    return value;
  }

  const CONTENT_THEMES = ['alwaysLight', 'followApp', 'alwaysDark'];

  function loadContentTheme() {
    try {
      const value = localStorage.getItem(CONTENT_THEME_KEY) || 'alwaysLight';
      return CONTENT_THEMES.includes(value) ? value : 'alwaysLight';
    } catch {
      return 'alwaysLight';
    }
  }

  function saveContentTheme(value) {
    state.contentTheme = CONTENT_THEMES.includes(value) ? value : 'alwaysLight';
    try {
      localStorage.setItem(CONTENT_THEME_KEY, state.contentTheme);
    } catch {
      /* ignore */
    }
    applyContentTheme();
  }

  function resolvedContentTheme() {
    if (state.contentTheme === 'alwaysDark') return 'dark';
    if (state.contentTheme === 'followApp') {
      return document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light';
    }
    return 'light';
  }

  function applyContentTheme() {
    document.documentElement.dataset.contentTheme = resolvedContentTheme();
  }

  function loadNavPane() {
    try {
      return localStorage.getItem(NAV_PANE_KEY) !== '0';
    } catch {
      return true;
    }
  }

  function setNavPane(open) {
    state.navPaneOpen = Boolean(open);
    $('#app').classList.toggle('with-result-nav', state.navPaneOpen);
    el.navToggle.setAttribute('aria-pressed', String(state.navPaneOpen));
    const title = state.navPaneOpen ? '隐藏结果导航 (Ctrl+Shift+N)' : '显示结果导航 (Ctrl+Shift+N)';
    el.navToggle.title = title;
    el.navToggle.setAttribute('aria-label', title);
    try {
      localStorage.setItem(NAV_PANE_KEY, state.navPaneOpen ? '1' : '0');
    } catch {
      /* ignore */
    }
  }

  function loadSidebar() {
    try {
      return localStorage.getItem(SIDEBAR_KEY) !== '0';
    } catch {
      return true;
    }
  }

  function setSidebar(open) {
    state.sidebarOpen = Boolean(open);
    $('#app').classList.toggle('without-sidebar', !state.sidebarOpen);
    el.sidebarToggle.setAttribute('aria-pressed', String(state.sidebarOpen));
    el.sidebarReveal.hidden = state.sidebarOpen;
    const title = state.sidebarOpen ? '隐藏词典侧栏 (Ctrl+B)' : '显示词典侧栏 (Ctrl+B)';
    el.sidebarToggle.title = title;
    el.sidebarToggle.setAttribute('aria-label', title);
    try {
      localStorage.setItem(SIDEBAR_KEY, state.sidebarOpen ? '1' : '0');
    } catch {
      /* ignore */
    }
  }

  function baseCss() {
    const size = Number(state.settings.fontSize) || 16;
    const phoneticFont = new URL('fonts/KingsoftPhoneticPlain.ttf', window.location.href).href;
    const dark = resolvedContentTheme() === 'dark';
    const vars = dark
      ? '--doc-bg:#1c1f27;--doc-text:#e9e5da;--doc-link:#f09472;--doc-hr:#2a2e38'
      : '--doc-bg:#fff;--doc-text:#1d2330;--doc-link:#a8462a;--doc-hr:#e7e0d4';
    return `
@font-face{font-family:"Kingsoft Phonetic Plain";src:url("${phoneticFont}") format("truetype");font-display:swap}
@font-face{font-family:"Kingsoft Phonetic";src:url("${phoneticFont}") format("truetype");font-display:swap}
:root{color-scheme:${dark ? 'dark' : 'light'};${vars}}
html{font-size:${size}px}
html,body{margin:0;padding:0;background:var(--doc-bg);color:var(--doc-text)}
body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Hiragino Sans GB","Microsoft YaHei","Noto Sans CJK SC",sans-serif;font-size:1rem;line-height:1.68;overflow-wrap:break-word;word-break:break-word;-webkit-text-size-adjust:100%}
.moedict-entry{padding:14px 18px 20px}
img,svg,video{max-width:100%;height:auto}
table{max-width:100%;border-collapse:collapse}
pre{max-width:100%;overflow:auto}
a{color:var(--doc-link)}
hr{border:0;border-top:1px solid var(--doc-hr);margin:1em 0}
.moedict-anchor-target{outline:2px solid var(--doc-link);outline-offset:2px;border-radius:2px;animation:moedict-anchor-hit 1.4s ease-out}
@keyframes moedict-anchor-hit{0%{background-color:rgba(168,70,42,.16)}100%{background-color:transparent}}
`;
  }

  function contentOverrideCss() {
    if (resolvedContentTheme() !== 'dark') return '';
    return `
html,body{background:var(--doc-bg)!important;color:var(--doc-text)!important}
body,div,p,li,td,th,section,article,header,footer,span{background-color:transparent!important;color:inherit!important}
a{color:var(--doc-link)!important}
hr{border-color:var(--doc-hr)!important}
table,td,th{border-color:var(--doc-hr)!important}
`;
  }

  /* ============================== theme / history ============================== */

  const darkQuery = window.matchMedia('(prefers-color-scheme: dark)');

  function applyTheme() {
    const choice = state.settings.theme || 'system';
    const resolved =
      choice === 'system' ? (darkQuery.matches ? 'dark' : 'light') : choice;
    document.documentElement.dataset.theme = resolved;
    applyContentTheme();
  }

  function loadHistory() {
    try {
      const raw = JSON.parse(localStorage.getItem(HISTORY_KEY) || '[]');
      return Array.isArray(raw)
        ? raw.filter((item) => typeof item === 'string' && item.trim()).slice(0, 24)
        : [];
    } catch {
      return [];
    }
  }

  function rememberSearch(word) {
    const key = word.toLowerCase();
    state.history = [word, ...state.history.filter((item) => item.toLowerCase() !== key)].slice(0, 24);
    try {
      localStorage.setItem(HISTORY_KEY, JSON.stringify(state.history));
    } catch {
      /* ignore */
    }
    if (!state.navigating && state.navStack[state.navIndex] !== word) {
      state.navStack = state.navStack.slice(0, state.navIndex + 1);
      state.navStack.push(word);
      if (state.navStack.length > 50) state.navStack.shift();
      state.navIndex = state.navStack.length - 1;
    }
    updateHistoryButtons();
    renderRecent();
  }

  function updateHistoryButtons() {
    el.historyBack.disabled = state.navIndex <= 0;
    el.historyForward.disabled = state.navIndex >= state.navStack.length - 1;
  }

  function navigateHistory(delta) {
    const next = state.navIndex + delta;
    if (next < 0 || next >= state.navStack.length) return;
    state.navIndex = next;
    state.navigating = true;
    updateHistoryButtons();
    runSearch(state.navStack[next]).finally(() => {
      state.navigating = false;
      updateHistoryButtons();
    });
  }

  function clearHistory() {
    state.history = [];
    state.navStack = [];
    state.navIndex = -1;
    updateHistoryButtons();
    try {
      localStorage.removeItem(HISTORY_KEY);
    } catch {
      /* ignore */
    }
    renderRecent();
  }

  function renderRecent() {
    if (!state.history.length) {
      el.recent.hidden = true;
      el.recentList.replaceChildren();
      return;
    }
    el.recent.hidden = false;
    el.recentList.replaceChildren();
    for (const word of state.history) {
      const chip = document.createElement('button');
      chip.className = 'chip';
      chip.textContent = word;
      chip.title = `查询 ${word}`;
      chip.addEventListener('click', () => runSearch(word));
      el.recentList.append(chip);
    }
  }

  /* ============================== dictionary list ============================== */

  function statusMeta(dict) {
    const live = state.indexing.get(dict.id);
    if (live) {
      const label =
        live.phase === 'writing'
          ? '写入索引…'
          : `索引中 ${Math.min(100, Math.round(live.percent || 0))}%`;
      return { label, cls: 'live', title: '正在建立索引' };
    }
    switch (dict.status) {
      case 'ready':
        return { label: `${formatNumber(dict.entryCount)} 条`, cls: 'ok' };
      case 'stale':
        return {
          label: '文件已更新，待重建',
          cls: 'warn',
          title: '词典文件已变化，点击 ⋯ 重建索引',
        };
      case 'unindexed':
        return {
          label: dict.enabled ? '等待建立索引' : '未建立索引',
          cls: '',
          title: '尚未建立索引',
        };
      case 'missing':
        return { label: '文件不存在', cls: 'err', title: dict.path };
      default:
        return {
          label: '索引异常',
          cls: 'err',
          title: dict.error || '索引文件损坏，可尝试重建',
        };
    }
  }

  function isDictFiltered() {
    return Boolean(state.filter.trim() || state.tagFilter);
  }

  function activeTags() {
    return state.tagFilter ? [state.tagFilter] : [];
  }

  function dictRow(dict) {
    const row = document.createElement('div');
    row.className = 'dict-row' + (dict.enabled ? '' : ' muted-dict');
    row.dataset.id = dict.id;
    row.setAttribute('role', 'listitem');
    row.tabIndex = 0;
    row.draggable = false;
    if (state.indexing.has(dict.id)) row.classList.add('indexing');

    const tags = Array.isArray(dict.tags) ? dict.tags : [];
    const tagsLine = tags.length
      ? `<div class="dict-tags-line">${tags.map(() => '<span class="dict-tag"></span>').join('')}</div>`
      : '';
    row.innerHTML = `
      <span class="dict-drag" data-role="drag" title="拖动排序">${icon('grip')}</span>
      <input class="dict-check" type="checkbox" ${dict.enabled ? 'checked' : ''} aria-label="启用词典">
      <div class="dict-main">
        <div class="dict-name-line">
          <span class="dict-name"></span>
          ${dict.hasMdd ? '<span class="badge">MDD</span>' : ''}
        </div>
        <div class="dict-meta"><span class="dot"></span><span class="meta-text"></span></div>
        ${tagsLine}
      </div>
      <button class="dict-more" data-role="more" title="词典操作" aria-label="词典操作">${icon('more')}</button>
      <div class="dict-progress" hidden><span></span></div>
    `;

    const nameEl = row.querySelector('.dict-name');
    nameEl.textContent = dict.name || '未命名词典';
    nameEl.title = dict.path || '';
    const meta = statusMeta(dict);
    row.querySelector('.meta-text').textContent = meta.label;
    const dot = row.querySelector('.dot');
    if (meta.cls) dot.classList.add(meta.cls);
    if (meta.title) row.querySelector('.dict-meta').title = meta.title;
    if (meta.cls === 'err' && dict.error) row.querySelector('.dict-meta').title = dict.error;

    const tagNodes = row.querySelectorAll('.dict-tag');
    tags.forEach((tag, index) => {
      tagNodes[index].textContent = tag;
    });
    const handle = row.querySelector('[data-role="drag"]');
    handle.draggable = state.dicts.length > 1 && !isDictFiltered();

    const live = state.indexing.get(dict.id);
    if (live) {
      const progress = row.querySelector('.dict-progress');
      progress.hidden = false;
      progress.querySelector('span').style.width = `${Math.max(2, Math.round(live.percent || 0))}%`;
    }
    return row;
  }

  function renderTagChips() {
    const tags = [...new Set(state.dicts.flatMap((dict) => dict.tags || []))].sort((a, b) =>
      a.localeCompare(b, 'zh-CN')
    );
    if (state.tagFilter && !tags.includes(state.tagFilter)) state.tagFilter = '';
    el.dictTags.replaceChildren();
    for (const [tag, label] of [['', '全部'], ...tags.map((tag) => [tag, tag])]) {
      const chip = document.createElement('button');
      chip.type = 'button';
      chip.className = 'tag-chip' + (state.tagFilter === tag ? ' active' : '');
      chip.dataset.tag = tag;
      chip.textContent = label;
      el.dictTags.append(chip);
    }
  }

  function renderDicts() {
    const filter = state.filter.trim().toLowerCase();
    const visible = state.dicts.filter((dict) => {
      if (state.tagFilter && !(dict.tags || []).includes(state.tagFilter)) return false;
      if (!filter) return true;
      return (
        (dict.name || '').toLowerCase().includes(filter) ||
        (dict.path || '').toLowerCase().includes(filter) ||
        (dict.tags || []).some((tag) => tag.toLowerCase().includes(filter))
      );
    });

    renderTagChips();
    el.dictList.replaceChildren();
    if (!visible.length && state.dicts.length) {
      const empty = document.createElement('div');
      empty.className = 'dict-filter-empty';
      empty.textContent = '没有匹配的词典';
      el.dictList.append(empty);
    }
    for (const dict of visible) el.dictList.append(dictRow(dict));
    el.dictEmpty.hidden = state.dicts.length > 0;
    el.dictFilter.disabled = state.dicts.length === 0;
  }

  function updateDictRow(id) {
    const dict = state.dicts.find((item) => item.id === id);
    const row = $$('.dict-row', el.dictList).find((item) => item.dataset.id === id);
    if (!dict || !row) return;
    const live = state.indexing.get(id);
    const meta = statusMeta(dict);
    const dot = row.querySelector('.dot');
    dot.className = `dot${meta.cls ? ` ${meta.cls}` : ''}`;
    row.querySelector('.meta-text').textContent = meta.label;
    if (meta.title) row.querySelector('.dict-meta').title = meta.title;
    const progress = row.querySelector('.dict-progress');
    progress.hidden = !live;
    if (live) {
      progress.querySelector('span').style.width = `${Math.max(2, Math.round(live.percent || 0))}%`;
    }
    row.classList.toggle('indexing', Boolean(live));
  }

  function updateSummary() {
    const total = state.dicts.length;
    const enabled = state.dicts.filter((dict) => dict.enabled).length;
    el.dictSummary.textContent = total ? `${enabled}/${total} 启用` : '0 本词典';
  }

  function updateStatus() {
    const total = state.dicts.length;
    const enabled = state.dicts.filter((dict) => dict.enabled).length;
    el.statusDicts.textContent = total ? `${enabled}/${total} 词典启用` : '未添加词典';

    let active = null;
    for (const progress of state.indexing.values()) {
      if (!active || progress.percent > active.percent) active = progress;
    }
    if (active) {
      el.statusIndex.hidden = false;
      const phase = active.phase === 'writing' ? '写入索引' : '索引中';
      el.statusIndex.textContent = `${phase} · ${active.name} ${Math.round(active.percent || 0)}%`;
    } else {
      el.statusIndex.hidden = true;
      el.statusIndex.textContent = '';
    }
  }

  /* ============================== welcome ============================== */

  function renderWelcome() {
    if (state.query) {
      el.welcome.hidden = true;
      return;
    }
    el.welcome.hidden = false;
    const enabled = state.dicts.filter((dict) => dict.enabled).length;
    let actions = '';
    if (!state.dicts.length) {
      actions = `
        <button class="btn btn-primary" data-action="pick-files">${icon('book')}添加 MDX 文件</button>
        <button class="btn btn-ghost" data-action="pick-folder">${icon('folder')}扫描文件夹</button>`;
    } else if (!enabled) {
      actions = `
        <button class="btn btn-primary" data-action="enable-all">${icon('check')}启用全部词典</button>
        <button class="btn btn-ghost" data-action="settings">${icon('settings')}词典设置</button>`;
    } else {
      actions = `
        <button class="btn btn-ghost" data-action="pick-files">${icon('plus')}添加词典</button>
        <button class="link-btn" data-action="lucky">随机一个词</button>`;
    }
    el.welcomeActions.innerHTML = actions;

    const subtitle = el.welcome.querySelector('.welcome-sub');
    if (!state.dicts.length) {
      subtitle.textContent = '从一个 .mdx 文件开始，或扫描整个词典文件夹。';
    } else if (!enabled) {
      subtitle.textContent = '所有词典都已停用，启用后即可查询。';
    } else {
      subtitle.textContent = `已启用 ${enabled} 本词典，输入单词或短语开始查询。`;
    }
    renderRecent();
  }

  function resetToWelcome() {
    state.query = '';
    state.hits = [];
    state.querySeq += 1;
    el.searchInput.value = '';
    toggleClearBtn();
    hideSuggestions();
    clearObservers();
    el.resultsList.replaceChildren();
    el.resultsMeta.hidden = true;
    el.resultNav.hidden = true;
    el.resultNav.replaceChildren();
    el.noResults.hidden = true;
    el.noResults.replaceChildren();
    el.searchLoading.hidden = true;
    el.statusQuery.textContent = '';
    el.welcome.hidden = false;
    renderWelcome();
  }

  /* ============================== data loading ============================== */

  async function refreshDicts() {
    if (state.dragId) return;
    try {
      const dicts = await invoke('list_dictionaries');
      state.dicts = Array.isArray(dicts) ? dicts : [];
    } catch (error) {
      toast(`读取词典列表失败：${error}`, 'error');
      return;
    }
    renderDicts();
    updateSummary();
    updateStatus();
    renderWelcome();
  }

  function scheduleRefresh() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => refreshDicts(), 150);
  }

  async function enableAll() {
    const disabled = state.dicts.filter((dict) => !dict.enabled);
    if (!disabled.length) return;
    await Promise.all(
      disabled.map((dict) =>
        invoke('set_dictionary_enabled', { id: dict.id, enabled: true }).catch(() => null)
      )
    );
    await refreshDicts();
  }

  async function toggleDict(id, enabled) {
    const dict = state.dicts.find((item) => item.id === id);
    if (dict) dict.enabled = enabled;
    updateSummary();
    updateStatus();
    try {
      await invoke('set_dictionary_enabled', { id, enabled });
    } catch (error) {
      if (dict) dict.enabled = !enabled;
      renderDicts();
      toast(`操作失败：${error}`, 'error');
    }
  }

  function moveDict(id, delta) {
    const index = state.dicts.findIndex((dict) => dict.id === id);
    const target = index + delta;
    if (index < 0 || target < 0 || target >= state.dicts.length) return;
    const next = state.dicts.slice();
    const [item] = next.splice(index, 1);
    next.splice(target, 0, item);
    state.dicts = next;
    renderDicts();
    persistOrder(next.map((dict) => dict.id));
  }

  function moveDictTo(id, target) {
    const index = state.dicts.findIndex((dict) => dict.id === id);
    if (index < 0 || target < 0 || target >= state.dicts.length || index === target) return;
    const next = state.dicts.slice();
    const [item] = next.splice(index, 1);
    next.splice(target, 0, item);
    state.dicts = next;
    renderDicts();
    persistOrder(next.map((dict) => dict.id));
    toast(`已移动到第 ${target + 1} 位`, 'success', 1600);
  }

  async function persistOrder(ids) {
    try {
      await invoke('reorder_dictionaries', { ids });
    } catch (error) {
      toast(`保存排序失败：${error}`, 'error');
      await refreshDicts();
    }
  }

  async function addPaths(paths) {
    if (!Array.isArray(paths) || !paths.length) return;
    try {
      const outcome = await invoke('add_dictionary_paths', { paths });
      await handleOutcome(outcome);
    } catch (error) {
      toast(`添加失败：${error}`, 'error');
    }
  }

  async function handleOutcome(outcome) {
    if (!outcome) return;
    const added = Array.isArray(outcome.added) ? outcome.added : [];
    const skipped = Array.isArray(outcome.skipped) ? outcome.skipped : [];
    if (added.length) {
      toast(`已添加 ${added.length} 本词典`, 'success');
      await refreshDicts();
    }
    if (skipped.length) {
      toast(
        `${skipped.length} 个路径未添加：${skipped[0].reason || '未知原因'}`,
        'warn',
        5200
      );
    }
  }

  async function pickFiles() {
    closePopovers();
    try {
      const outcome = await invoke('pick_dictionary_files');
      await handleOutcome(outcome);
    } catch (error) {
      toast(`添加失败：${error}`, 'error');
    }
  }

  async function pickFolder() {
    closePopovers();
    try {
      const outcome = await invoke('pick_dictionary_folder');
      await handleOutcome(outcome);
    } catch (error) {
      toast(`扫描失败：${error}`, 'error');
    }
  }

  /* ============================== search ============================== */

  function toggleClearBtn() {
    el.clearBtn.hidden = !el.searchInput.value;
  }

  function focusSearch() {
    el.searchInput.focus();
    el.searchInput.select();
  }

  function onSearchInput() {
    toggleClearBtn();
    const value = el.searchInput.value.trim();
    clearTimeout(suggestTimer);
    if (!value) {
      state.suggestSeq += 1;
      hideSuggestions();
      return;
    }
    suggestTimer = setTimeout(() => fetchSuggestions(value), 140);
  }

  async function fetchSuggestions(prefix) {
    const seq = ++state.suggestSeq;
    try {
      const items = await invoke('suggest', { prefix, tags: activeTags() });
      if (seq !== state.suggestSeq) return;
      state.suggestions = Array.isArray(items) ? items : [];
      state.suggestionIndex = -1;
      renderSuggestions(prefix);
    } catch {
      if (seq === state.suggestSeq) hideSuggestions();
    }
  }

  function renderSuggestions(prefix) {
    if (!state.suggestions.length) {
      hideSuggestions();
      return;
    }
    el.suggestions.replaceChildren();
    const lowerPrefix = prefix.toLowerCase();
    const limit = Math.max(1, Number(state.settings.suggestionLimit) || 12);
    state.suggestions.slice(0, limit).forEach((item, index) => {
      const row = document.createElement('div');
      row.className = 'suggestion';
      row.dataset.word = item.word;
      row.setAttribute('role', 'option');
      row.dataset.index = String(index);

      const word = document.createElement('span');
      word.className = 'suggestion-word';
      const lowered = item.word.toLowerCase();
      if (lowered.startsWith(lowerPrefix)) {
        const mark = document.createElement('mark');
        mark.textContent = item.word.slice(0, prefix.length);
        word.append(mark, document.createTextNode(item.word.slice(prefix.length)));
      } else {
        word.textContent = item.word;
      }

      const dicts = document.createElement('span');
      dicts.className = 'suggestion-dicts';
      dicts.textContent = (item.dicts || []).join(' · ');
      dicts.title = dicts.textContent;
      row.append(word, dicts);
      el.suggestions.append(row);
    });
    el.suggestions.hidden = false;
  }

  function hideSuggestions() {
    el.suggestions.hidden = true;
    el.suggestions.replaceChildren();
    state.suggestions = [];
    state.suggestionIndex = -1;
  }

  function moveSuggestion(delta) {
    if (el.suggestions.hidden || !state.suggestions.length) return;
    const rows = $$('.suggestion', el.suggestions);
    state.suggestionIndex =
      (state.suggestionIndex + delta + rows.length) % rows.length;
    rows.forEach((row, index) =>
      row.classList.toggle('active', index === state.suggestionIndex)
    );
    rows[state.suggestionIndex]?.scrollIntoView({ block: 'nearest' });
  }

  function chooseSuggestion(index = state.suggestionIndex) {
    const item = state.suggestions[index];
    if (!item) return false;
    runSearch(item.word);
    return true;
  }

  function onSearchKeydown(event) {
    if (event.key === 'Enter') {
      event.preventDefault();
      if (!chooseSuggestion()) {
        const word = el.searchInput.value.trim();
        if (word) runSearch(word);
      }
      return;
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (el.suggestions.hidden && el.searchInput.value.trim()) {
        fetchSuggestions(el.searchInput.value.trim());
      } else {
        moveSuggestion(1);
      }
      return;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveSuggestion(-1);
      return;
    }
    if (event.key === 'Tab' && !el.suggestions.hidden && state.suggestionIndex >= 0) {
      event.preventDefault();
      const item = state.suggestions[state.suggestionIndex];
      if (item) {
        el.searchInput.value = item.word;
        toggleClearBtn();
        hideSuggestions();
      }
      return;
    }
    if (event.key === 'Escape') {
      if (!el.suggestions.hidden) {
        event.preventDefault();
        event.stopPropagation();
        hideSuggestions();
      }
    }
  }

  async function runSearch(rawWord, rawAnchor = '', rawDictId = '') {
    const word = String(rawWord ?? '').trim();
    if (!word) return;
    const anchor = String(rawAnchor || '');
    const anchorDict = String(rawDictId || '');

    clearTimeout(suggestTimer);
    state.suggestSeq += 1;
    hideSuggestions();
    closePopovers();
    el.searchInput.value = word;
    toggleClearBtn();

    const seq = ++state.querySeq;
    focusSeq += 1;
    state.query = word;
    state.hits = [];
    clearObservers();
    el.searchLoading.hidden = false;
    el.welcome.hidden = true;
    el.noResults.hidden = true;
    el.noResults.replaceChildren();
    el.resultsList.replaceChildren();
    el.resultsMeta.hidden = true;
    el.resultNav.hidden = true;
    el.resultNav.replaceChildren();
    el.statusQuery.textContent = '';
    el.results.scrollTop = 0;

    const started = performance.now();
    try {
      const hits = await invoke('query', { word, tags: activeTags() });
      if (seq !== state.querySeq) return;
      const elapsed = performance.now() - started;
      const list = Array.isArray(hits) ? hits : [];
      state.hits = list;
      renderResults(list, word, elapsed);
      if (anchor) applyEntryAnchor(anchor, anchorDict);
    } catch (error) {
      if (seq !== state.querySeq) return;
      showNoResults(word, performance.now() - started, `查询失败：${error}`);
    } finally {
      if (seq === state.querySeq) {
        el.searchLoading.hidden = true;
        // Keep the box ready for the next word: select the query when the
        // search was issued from the input itself.
        if (document.activeElement === el.searchInput && el.searchInput.value === word) {
          el.searchInput.select();
        }
      }
    }
  }

  function applyEntryAnchor(anchor, dictId) {
    if (!anchor) return;
    const cards = $$('.result-card', el.resultsList);
    if (!cards.length) return;
    const card =
      (dictId && cards.find((item) => item.dataset.dictId === dictId)) || cards[0];
    if (card.classList.contains('collapsed')) {
      card.classList.remove('collapsed');
      card.querySelector('[data-action="collapse"]')?.setAttribute('aria-expanded', 'true');
    }
    ensureFrame(card).then(() => {
      if (card.isConnected) scrollToEntryAnchor(card, anchor);
    });
  }

  function renderResults(hits, word, elapsed) {
    el.searchLoading.hidden = true;
    el.welcome.hidden = true;
    if (!hits.length) {
      showNoResults(word, elapsed);
      return;
    }

    clearObservers();
    el.resultsList.replaceChildren();
    el.resultNav.hidden = false;
    renderResultNav(hits);
    ensureCardObserver();
    const expandedLimit = Math.max(0, Number(state.settings.defaultExpanded ?? 3));
    hits.forEach((hit, index) => {
      const card = resultCard(hit);
      if (index >= expandedLimit) {
        card.classList.add('collapsed');
        card.querySelector('[data-action="collapse"]')?.setAttribute('aria-expanded', 'false');
      }
      el.resultsList.append(card);
      cardObserver.observe(card);
    });

    if (state.settings.showAllDicts) {
      const hitIds = new Set(hits.map((hit) => hit.dictId));
      for (const dict of state.dicts) {
        if (!dict.enabled || hitIds.has(dict.id)) continue;
        el.resultsList.append(missingCard(dict));
      }
    }

    const indexing = state.dicts.filter(
      (dict) => dict.enabled && state.indexing.has(dict.id)
    ).length;
    const parts = [`在 ${hits.length} 本词典中找到结果`, formatDuration(elapsed)];
    if (indexing) parts.push(`${indexing} 本索引进行中`);
    el.resultsMeta.innerHTML = `
      <span>${escapeHtml(parts.join(' · '))}</span>
      <span class="spacer"></span>
      <button class="mini-btn" data-action="expand-all">全部展开</button>
      <button class="mini-btn" data-action="collapse-all">全部收起</button>
    `;
    el.resultsMeta.hidden = false;
    el.statusQuery.textContent = `${hits.length} 条结果 · ${formatDuration(elapsed)}`;
    rememberSearch(word);
    el.results.scrollTop = 0;
  }

  function renderResultNav(hits) {
    el.resultNav.replaceChildren();
    const all = document.createElement('button');
    all.className = 'nav-chip nav-all';
    all.textContent = `全部 ${hits.length}`;
    all.addEventListener('click', () => scrollResultsTo(0));
    el.resultNav.append(all);

    for (const hit of hits) {
      const chip = document.createElement('button');
      chip.className = 'nav-chip';
      chip.dataset.dictId = hit.dictId;
      chip.innerHTML = '<span class="nav-dot"></span><span class="nav-name"></span>';
      chip.querySelector('.nav-name').textContent = hit.dictName || '词典';
      chip.title = hit.dictName || '';
      chip.addEventListener('click', () => focusDictionary(hit.dictId));
      el.resultNav.append(chip);
    }

    if (state.settings.showAllDicts) {
      const hitIds = new Set(hits.map((hit) => hit.dictId));
      for (const dict of state.dicts) {
        if (!dict.enabled || hitIds.has(dict.id)) continue;
        const chip = document.createElement('button');
        chip.className = 'nav-chip nav-missing';
        chip.dataset.dictId = dict.id;
        chip.innerHTML = '<span class="nav-dot"></span><span class="nav-name"></span>';
        chip.querySelector('.nav-name').textContent = dict.name || '词典';
        chip.title = `${dict.name || '词典'}：未收录`;
        chip.addEventListener('click', () => {
          const card = $$('.result-card', el.resultsList).find(
            (item) => item.dataset.dictId === dict.id
          );
          if (card) focusDictionary(dict.id);
          else toast('当前结果中没有这本词典', 'info', 1800);
        });
        el.resultNav.append(chip);
      }
    }
    updateActiveNav();
  }

  let navScrollPending = false;

  function scheduleNavUpdate() {
    if (navScrollPending) return;
    navScrollPending = true;
    requestAnimationFrame(() => {
      navScrollPending = false;
      updateActiveNav();
    });
  }

  function updateActiveNav() {
    const cards = $$('.result-card', el.resultsList);
    if (!cards.length) return;
    const threshold = el.results.getBoundingClientRect().top + 48;
    let activeId = cards[0].dataset.dictId;
    for (const card of cards) {
      if (card.getBoundingClientRect().top <= threshold) activeId = card.dataset.dictId;
    }
    $$('.nav-chip[data-dict-id]', el.resultNav).forEach((chip) => {
      const active = chip.dataset.dictId === activeId;
      chip.classList.toggle('active', active);
      if (active) chip.setAttribute('aria-current', 'true');
      else chip.removeAttribute('aria-current');
    });
  }

  async function showNoResults(word, elapsed, note) {
    clearObservers();
    el.resultsList.replaceChildren();
    el.resultsMeta.hidden = true;
    el.resultNav.hidden = true;
    el.resultNav.replaceChildren();
    el.welcome.hidden = true;
    el.noResults.hidden = false;
    el.noResults.replaceChildren();

    const enabled = state.dicts.filter(
      (dict) =>
        dict.enabled && (!state.tagFilter || (dict.tags || []).includes(state.tagFilter))
    ).length;
    const wrap = document.createElement('div');
    wrap.innerHTML = `
      <div class="no-mark">${icon('search')}</div>
      <h2></h2>
      <p></p>
      <div class="chips"></div>
      <div class="welcome-actions"></div>
    `;
    const heading = wrap.querySelector('h2');
    const paragraph = wrap.querySelector('p');
    const chips = wrap.querySelector('.chips');
    const actions = wrap.querySelector('.welcome-actions');

    if (!enabled) {
      heading.textContent = '还没有启用的词典';
      paragraph.textContent = '在左侧勾选词典，或者先添加一本 MDX。';
      actions.innerHTML = `
        <button class="btn btn-primary" data-action="pick-files">${icon('book')}添加 MDX 文件</button>
        <button class="btn btn-ghost" data-action="pick-folder">${icon('folder')}扫描文件夹</button>`;
    } else {
      heading.textContent = `没有找到「${word}」`;
      paragraph.textContent =
        note ||
        (state.tagFilter
          ? `当前标签「${state.tagFilter}」内没有命中，可切换标签或勾选更多词典。`
          : '换一个拼写试试，或者从下面的联想词中选择。部分词典可能仍在建立索引。');
      actions.innerHTML = `
        <button class="link-btn" data-action="lucky">随机一个词</button>`;
    }

    el.noResults.append(wrap);
    el.statusQuery.textContent = enabled
      ? `未找到 · ${formatDuration(elapsed)}`
      : '没有启用的词典';

    if (!enabled) return;

    const prefix = Array.from(word).slice(0, 2).join('');
    if (!prefix) return;
    try {
      const items = await invoke('suggest', { prefix, tags: activeTags() });
      if (state.query !== word) return;
      const filtered = (Array.isArray(items) ? items : [])
        .filter((item) => item.word.toLowerCase() !== word.toLowerCase())
        .slice(0, 10);
      for (const item of filtered) {
        const chip = document.createElement('button');
        chip.className = 'chip';
        chip.textContent = item.word;
        chip.title = (item.dicts || []).join(' · ');
        chip.addEventListener('click', () => runSearch(item.word));
        chips.append(chip);
      }
    } catch {
      /* suggestions are best-effort */
    }
  }

  /* ============================== result cards ============================== */

  function resultCard(hit) {
    const card = document.createElement('article');
    card.className = 'result-card';
    card.dataset.dictId = hit.dictId;
    card._hit = hit;
    card.innerHTML = `
      <header class="result-head">
        <div class="result-dict"><span class="book-dot"></span><span></span></div>
        <div class="result-headword"></div>
        <div class="result-actions">
          <button class="icon-btn" data-action="copy" title="复制释义" aria-label="复制释义">${icon('copy')}</button>
          <button class="icon-btn" data-action="collapse" title="收起/展开" aria-label="收起或展开" aria-expanded="true">${icon('chevron')}</button>
        </div>
      </header>
      <div class="result-body">
        <div class="result-placeholder">
          <div class="skeleton-line"></div>
          <div class="skeleton-line"></div>
          <div class="skeleton-line"></div>
        </div>
      </div>
    `;
    card.querySelector('.result-dict span:last-child').textContent = hit.dictName || '词典';
    const headword = card.querySelector('.result-headword');
    headword.textContent = hit.headword || '';
    headword.title = hit.headword || '';
    const chevron = card.querySelector('[data-action="collapse"] .icon');
    if (chevron) chevron.classList.add('chevron');
    return card;
  }

  function missingCard(dict) {
    const card = document.createElement('article');
    card.className = 'result-card';
    card.dataset.dictId = dict.id;
    let note = '未收录该词';
    if (state.indexing.has(dict.id)) note = '正在建立索引…';
    else if (dict.status === 'stale') note = '词典已更新，正在等待重建索引';
    else if (dict.status === 'unindexed') note = '尚未建立索引';
    else if (dict.status === 'missing') note = '词典文件不存在';
    else if (dict.status === 'error') note = dict.error || '索引异常';
    card.innerHTML = `
      <header class="result-head">
        <div class="result-dict"><span class="book-dot"></span><span></span></div>
        <span class="result-count">${escapeHtml(note)}</span>
      </header>
      <div class="missing-card"></div>
    `;
    card.querySelector('.result-dict span:last-child').textContent = dict.name || '词典';
    card.querySelector('.missing-card').textContent = note;
    return card;
  }

  function ensureCardObserver() {
    if (cardObserver) return;
    cardObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) loadCard(entry.target);
        }
      },
      { root: el.results, rootMargin: '420px 0px' }
    );
  }

  function clearObservers() {
    cardObserver?.disconnect();
    for (const observer of state.frameObservers) observer.disconnect();
    state.frameObservers.clear();
  }

  function loadCard(card) {
    if (!card || card.dataset.loaded) {
      if (card?._framePromise) return card._framePromise;
      return null;
    }
    card.dataset.loaded = '1';
    const hit = card._hit;
    const body = card.querySelector('.result-body');
    if (!body || !hit) return null;

    const iframe = document.createElement('iframe');
    iframe.className = 'entry-frame';
    iframe.setAttribute('sandbox', 'allow-same-origin');
    iframe.setAttribute('referrerpolicy', 'no-referrer');
    iframe.srcdoc = buildDocument(hit.html || '<p>（空释义）</p>', hit.dictId);

    card._framePromise = new Promise((resolve) => {
      card._resolveFrame = resolve;
    });

    iframe.addEventListener(
      'load',
      () => {
        card._frame = iframe;
        bindFrame(iframe, hit, card);
        const observer = sizeFrame(card, iframe);
        if (observer) state.frameObservers.add(observer);
        card._resolveFrame?.(iframe);
      },
      { once: true }
    );

    const wrap = document.createElement('div');
    wrap.className = 'frame-wrap';
    wrap.append(iframe);
    body.replaceChildren(wrap);
    return card._framePromise;
  }

  async function ensureFrame(card) {
    if (!card) return null;
    if (card._frame) return card._frame;
    if (!card._framePromise) loadCard(card);
    if (!card._framePromise) return null;
    return Promise.race([
      card._framePromise,
      new Promise((resolve) => setTimeout(() => resolve(card._frame || null), 1500)),
    ]);
  }

  function sizeFrame(card, iframe) {
    const doc = iframe.contentDocument;
    if (!doc) return null;
    const body = card.querySelector('.result-body');
    let expanded = false;

    let updatePending = false;
    const applyUpdate = () => {
      const height = Math.max(
        doc.documentElement?.scrollHeight || 0,
        doc.body?.scrollHeight || 0,
        60
      );
      iframe.style.height = `${height}px`;
      if (!body || expanded) return;
      if (height > TALL_THRESHOLD) {
        body.classList.add('tall');
        if (!body.querySelector('.expand-bar')) {
          const bar = document.createElement('div');
          bar.className = 'expand-bar';
          const button = document.createElement('button');
          button.className = 'expand-btn';
          button.textContent = '展开全文';
          button.addEventListener('click', () => {
            expanded = true;
            body.dataset.expanded = '1';
            body.classList.remove('tall');
            bar.remove();
          });
          bar.append(button);
          body.append(bar);
        }
      } else {
        body.classList.remove('tall');
        body.querySelector('.expand-bar')?.remove();
      }
    };
    const update = () => {
      if (updatePending) return;
      updatePending = true;
      requestAnimationFrame(() => {
        updatePending = false;
        applyUpdate();
      });
    };

    update();
    const observer =
      typeof ResizeObserver !== 'undefined' ? new ResizeObserver(update) : null;
    if (observer) {
      if (doc.body) observer.observe(doc.body);
      observer.observe(doc.documentElement);
    }
    doc.querySelectorAll('img').forEach((img) => img.addEventListener('load', update));
    return observer;
  }

  function bindFrame(iframe, hit, card) {
    const doc = iframe.contentDocument;
    if (!doc) return;

    doc.addEventListener(
      'click',
      (event) => {
        const target = event.target.closest?.('a, area, [data-sound]');
        if (!target) return;

        const sound = target.dataset?.sound;
        if (sound) {
          event.preventDefault();
          playSound(hit.dictId, sound);
          return;
        }

        const entry = target.dataset?.entry;
        if (entry) {
          event.preventDefault();
          const anchor = target.dataset?.entryAnchor || '';
          const word = decodeSafe(entry);
          // `entry://current-word#anchor` is a same-entry jump in disguise.
          if (anchor && word.toLowerCase() === state.query.toLowerCase() && card?._frame) {
            scrollToEntryAnchor(card, anchor);
            return;
          }
          runSearch(word, anchor, target.dataset?.entryDict || '');
          return;
        }

        if (target.hasAttribute?.('data-anchor')) {
          event.preventDefault();
          scrollToEntryAnchor(card, target.getAttribute('data-anchor') || '');
          return;
        }

        const external = target.dataset?.external;
        if (external) {
          event.preventDefault();
          invoke('open_external', { url: external }).catch((error) =>
            toast(`无法打开链接：${error}`, 'error')
          );
          return;
        }

        // Nothing else may navigate the frame: srcdoc content inherits the
        // app's base URL, so `#fragment` and relative hrefs would replace the
        // entry with the app's own page.
        event.preventDefault();
        const href = target.getAttribute?.('href') || '';
        if (href.startsWith('#')) {
          scrollToEntryAnchor(card, decodeSafe(href.slice(1)));
        }
      },
      true
    );

    doc.addEventListener(
      'dblclick',
      (event) => {
        const selection = doc.getSelection?.()?.toString().trim() || '';
        if (
          selection &&
          selection.length <= 48 &&
          !/[\r\n]/.test(selection) &&
          selection !== state.query
        ) {
          event.preventDefault();
          runSearch(selection);
        }
      },
      true
    );
  }

  function toggleCollapse(card) {
    if (!card || !card.querySelector('.result-body')) return;
    const collapsed = card.classList.toggle('collapsed');
    const button = card.querySelector('[data-action="collapse"]');
    if (button) button.setAttribute('aria-expanded', String(!collapsed));
    if (!collapsed) ensureFrame(card);
  }

  async function copyCard(card) {
    const hit = card?._hit;
    if (!hit) return;
    const frame = await ensureFrame(card);
    let text = '';
    if (frame?.contentDocument?.body) {
      text = frame.contentDocument.body.innerText || '';
    }
    if (!text) {
      const doc = new DOMParser().parseFromString(hit.html || '', 'text/html');
      text = doc.body?.textContent || '';
    }
    const payload = `${hit.headword || state.query}\n\n${text.trim()}`.trim();
    try {
      await navigator.clipboard.writeText(payload);
      toast('已复制释义', 'success', 1800);
    } catch {
      fallbackCopy(payload);
    }
  }

  function fallbackCopy(text) {
    const area = document.createElement('textarea');
    area.value = text;
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.append(area);
    area.select();
    let ok = false;
    try {
      ok = document.execCommand('copy');
    } catch {
      ok = false;
    }
    area.remove();
    toast(ok ? '已复制释义' : '复制失败', ok ? 'success' : 'error', 2000);
  }

  let resultsScrollFrame = 0;

  function prefersReducedMotion() {
    return Boolean(window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches);
  }

  function cancelResultsScroll() {
    if (!resultsScrollFrame) return;
    cancelAnimationFrame(resultsScrollFrame);
    resultsScrollFrame = 0;
  }

  /// Scrolls the results pane with a fixed duration so a jump between distant
  /// cards never stretches into a multi-second animation.
  function scrollResultsTo(top, { smooth = true, duration = 260 } = {}) {
    const scroller = el.results;
    if (!scroller) return;
    cancelResultsScroll();
    const max = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
    const target = Math.min(Math.max(0, Math.round(top)), max);
    const start = scroller.scrollTop;
    const delta = target - start;
    if (!smooth || prefersReducedMotion() || Math.abs(delta) < 2) {
      scroller.scrollTop = target;
      return;
    }
    const began = performance.now();
    const step = (now) => {
      const t = Math.min(1, (now - began) / duration);
      const eased = t * t * (3 - 2 * t);
      scroller.scrollTop = start + delta * eased;
      if (t < 1) {
        resultsScrollFrame = requestAnimationFrame(step);
      } else {
        resultsScrollFrame = 0;
      }
    };
    resultsScrollFrame = requestAnimationFrame(step);
  }

  function findAnchorNode(doc, anchor) {
    if (!doc || !anchor) return null;
    let node = doc.getElementById(anchor);
    if (node) return node;
    const named = doc.getElementsByName?.(anchor);
    if (named?.length) return named[0];
    // Legacy dictionaries sometimes differ only by case, or use `<a name>`.
    const wanted = anchor.toLowerCase();
    for (const candidate of doc.querySelectorAll('[id],[name]')) {
      const value =
        candidate.getAttribute('id') || candidate.getAttribute('name') || '';
      if (value.toLowerCase() === wanted) return candidate;
    }
    return null;
  }

  function flashAnchor(node) {
    if (!node?.classList) return;
    node.classList.remove('moedict-anchor-target');
    void node.offsetWidth;
    node.classList.add('moedict-anchor-target');
    setTimeout(() => node.classList.remove('moedict-anchor-target'), 1400);
  }

  /// Jumps to a named anchor inside an entry frame.  An empty or unknown
  /// anchor means the top of the entry (the usual "top" navigation link).
  function scrollToEntryAnchor(card, anchor) {
    if (!card) return;
    const body = card.querySelector('.result-body');
    if (body?.classList.contains('tall')) {
      body.querySelector('.expand-btn')?.click();
    }
    const frame = card._frame;
    const doc = frame?.contentDocument;

    // Keep the frame's `:target` state in sync without letting the frame
    // navigate: some dictionaries show/hide sections purely through CSS.
    try {
      const win = frame?.contentWindow;
      if (win) {
        const next = anchor ? `#${anchor}` : '';
        if (win.location.hash !== next) win.location.hash = next;
      }
    } catch {
      /* ignore: fall back to a plain scroll */
    }

    requestAnimationFrame(() => {
      if (!card.isConnected) return;
      const node = findAnchorNode(doc, anchor);
      let offset = 0;
      if (node && frame) {
        const frameRect = frame.getBoundingClientRect();
        const nodeRect = node.getBoundingClientRect();
        offset = Math.max(0, nodeRect.top - frameRect.top);
        flashAnchor(node);
      }
      const top =
        card.getBoundingClientRect().top -
        el.results.getBoundingClientRect().top +
        el.results.scrollTop +
        offset -
        12;
      scrollResultsTo(top);
    });
  }

  async function waitForStableLayout(cards) {
    let last = -1;
    let stable = 0;
    for (let attempt = 0; attempt < 18; attempt += 1) {
      await new Promise((resolve) => requestAnimationFrame(resolve));
      const total = cards.reduce((sum, card) => sum + card.getBoundingClientRect().height, 0);
      if (Math.abs(total - last) < 1) {
        stable += 1;
        if (stable >= 2) return;
      } else {
        stable = 0;
      }
      last = total;
    }
  }

  async function focusDictionary(id) {
    const seq = ++focusSeq;
    const cards = $$('.result-card', el.resultsList);
    const index = cards.findIndex((item) => item.dataset.dictId === id);
    if (index < 0) {
      toast('当前结果中没有这本词典', 'info', 1800);
      return;
    }
    const card = cards[index];
    const preceding = cards.slice(0, index + 1);
    for (const item of preceding) {
      if (!item.classList.contains('collapsed')) continue;
      item.classList.remove('collapsed');
      item.querySelector('[data-action="collapse"]')?.setAttribute('aria-expanded', 'true');
    }
    try {
      await Promise.all(preceding.map((item) => ensureFrame(item)));
      await waitForStableLayout(cards.slice(0, index + 1));
    } catch {
      /* scrolling still works with partially loaded frames */
    }
    // A newer jump or query may have won while frames were loading.
    if (seq !== focusSeq || !card.isConnected) return;
    const top =
      card.getBoundingClientRect().top -
      el.results.getBoundingClientRect().top +
      el.results.scrollTop -
      8;
    scrollResultsTo(top);
    card.classList.remove('flash');
    void card.offsetWidth;
    card.classList.add('flash');
  }

  /* ============================== definition document ============================== */

  function buildDocument(html, dictId) {
    const doc = new DOMParser().parseFromString(String(html || ''), 'text/html');
    sanitizeDocument(doc);

    const headParts = [];
    doc.querySelectorAll('style').forEach((style) => {
      style.textContent = rewriteCssUrls(style.textContent || '', dictId);
      headParts.push(style.outerHTML);
      style.remove();
    });
    doc.querySelectorAll('link').forEach((link) => {
      const rel = (link.getAttribute('rel') || '').toLowerCase();
      if (!rel.split(/\s+/).includes('stylesheet')) {
        link.remove();
        return;
      }
      const href = stripControls(link.getAttribute('href') || '');
      if (!href || /^(https?:)?\/\//i.test(href)) {
        link.remove();
        return;
      }
      link.setAttribute('href', resourceUrl(dictId, href));
      headParts.push(link.outerHTML);
      link.remove();
    });

    rewriteLinks(doc, dictId);
    rewriteResources(doc, dictId);

    const body = doc.body ? doc.body.innerHTML : '';
    return `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><style>${baseCss()}</style>${headParts.join(
      ''
    )}<style>${contentOverrideCss()}</style></head><body><div class="moedict-entry">${body}</div></body></html>`;
  }

  function sanitizeDocument(doc) {
    const audioSources = new Map();
    doc.querySelectorAll('audio[id][src]').forEach((node) => {
      const src = stripControls(node.getAttribute('src') || '');
      if (node.id && src) audioSources.set(node.id, src);
      node.setAttribute('preload', 'none');
    });
    const playCall = /document\s*\.\s*getElementById\s*\(\s*["']([^"']+)["']\s*\)\s*\.\s*play\s*\(\s*\)/i;
    const drop = [
      'script',
      'iframe',
      'frame',
      'frameset',
      'object',
      'embed',
      'applet',
      'form',
      'input',
      'button',
      'textarea',
      'select',
      'option',
      'base',
      'meta',
      'bgsound',
    ];
    for (const selector of drop) doc.querySelectorAll(selector).forEach((node) => node.remove());

    doc.querySelectorAll('audio,video').forEach((node) => {
      node.removeAttribute('autoplay');
      node.removeAttribute('loop');
    });

    for (const node of doc.querySelectorAll('*')) {
      for (const attr of Array.from(node.attributes)) {
        const name = attr.name.toLowerCase();
        const value = attr.value || '';
        if (name.startsWith('on')) {
          const match = value.match(playCall);
          const source = match && audioSources.get(match[1]);
          if (source && !node.dataset.sound) node.dataset.sound = source;
          node.removeAttribute(attr.name);
          continue;
        }
        if (name === 'srcdoc' || name === 'formaction') {
          node.removeAttribute(attr.name);
          continue;
        }
        if (name === 'style' && /expression\s*\(|javascript:/i.test(value)) {
          node.removeAttribute(attr.name);
          continue;
        }
        if (
          ['href', 'src', 'xlink:href', 'action'].includes(name) &&
          /^\s*(javascript|vbscript|data:text\/html)/i.test(value)
        ) {
          node.removeAttribute(attr.name);
        }
      }
    }
  }

  /// Splits an `entry://` target into entry name and `#fragment`.
  ///
  /// Some MDX builders percent-encode the delimiter (`entry://%23_hkn3`),
  /// which used to be mistaken for an entry named "#_hkn3".
  function splitEntryHref(value) {
    const hash = value.indexOf('#');
    if (hash >= 0) {
      return {
        target: decodeSafe(value.slice(0, hash)),
        anchor: decodeSafe(value.slice(hash + 1)),
      };
    }
    const encoded = value.search(/%23/i);
    if (encoded >= 0) {
      const target = decodeSafe(value.slice(0, encoded));
      const anchor = decodeSafe(value.slice(encoded + 3));
      // `word%23anchor` is an anchor link; `C%23` is a headword.
      if (!target || anchor) return { target, anchor };
    }
    return { target: decodeSafe(value), anchor: '' };
  }

  function rewriteLinks(doc, dictId) {
    // `<area>` links (image maps) are common in cover/navigation pages.
    doc.querySelectorAll('a[href], area[href]').forEach((link) => {
      const href = stripControls(link.getAttribute('href') || '');
      if (!href) {
        link.removeAttribute('href');
        return;
      }
      const entry = href.match(/^entry:\/\/(.*)$/i);
      if (entry) {
        const { target, anchor } = splitEntryHref(entry[1]);
        if (!target) {
          setAnchor(link, anchor);
          return;
        }
        const word = target.replace(/:[A-Za-z0-9_ ]+$/, '');
        if (!word) {
          setAnchor(link, anchor);
          return;
        }
        link.dataset.entry = word;
        if (anchor) link.dataset.entryAnchor = anchor;
        link.dataset.entryDict = dictId;
        link.setAttribute('href', '#');
        return;
      }
      const sound = href.match(/^sound:\/\/(.*)$/i);
      if (sound) {
        link.dataset.sound = decodeSafe(sound[1]);
        link.setAttribute('href', '#');
        return;
      }
      if (/^(https?|mailto):/i.test(href)) {
        link.dataset.external = href;
        link.setAttribute('target', '_blank');
        link.setAttribute('rel', 'noopener noreferrer');
        return;
      }
      if (href.startsWith('#')) {
        setAnchor(link, decodeSafe(href.slice(1)));
        return;
      }
      if (/^javascript:/i.test(href)) {
        link.removeAttribute('href');
        return;
      }
      if (/^[a-z][a-z0-9+.-]*:/i.test(href)) {
        link.removeAttribute('href');
        return;
      }
      if (!/[\\/:]/.test(href)) {
        const { target, anchor } = splitEntryHref(href);
        if (!target) {
          setAnchor(link, anchor);
          return;
        }
        link.dataset.entry = target;
        if (anchor) link.dataset.entryAnchor = anchor;
        link.dataset.entryDict = dictId;
        link.setAttribute('href', '#');
        return;
      }
      link.removeAttribute('href');
    });

    // Fragment links are handled by the host frame: `srcdoc` documents
    // inherit the app's base URL, so a raw `#fragment`/`href="#"` click would
    // navigate the entry frame to the whole app page.
    function setAnchor(link, anchor) {
      link.dataset.anchor = anchor;
      link.setAttribute('href', '#');
    }
  }

  function rewriteResources(doc, dictId) {
    const pairs = [
      ['img', 'src'],
      ['source', 'src'],
      ['audio', 'src'],
      ['video', 'src'],
      ['video', 'poster'],
      ['image', 'href'],
      ['use', 'href'],
    ];
    const rewriteAttr = (node, attr, dictId) => {
      let value = node.getAttribute(attr);
      if (value == null) return;
      value = stripControls(value);
      if (!value || value.startsWith('#')) return;
      if (/^(data:|blob:)/i.test(value)) return;
      if (/^(https?:)?\/\//i.test(value)) return;
      if (/^entry:\/\//i.test(value)) return;
      if (/^sound:\/\//i.test(value)) value = value.replace(/^sound:\/\//i, '');
      node.setAttribute(attr, resourceUrl(dictId, value));
    };
    for (const [tag, attr] of pairs) {
      doc.querySelectorAll(`${tag}[${attr}]`).forEach((node) =>
        rewriteAttr(node, attr, dictId)
      );
    }
    // `xlink:href` cannot be used in a CSS selector, so handle it manually.
    doc.querySelectorAll('image, use').forEach((node) => {
      if (node.hasAttribute('xlink:href')) {
        rewriteAttr(node, 'xlink:href', dictId);
      }
    });

    doc.querySelectorAll('img[srcset], source[srcset]').forEach((node) => {
      const srcset = node.getAttribute('srcset');
      if (!srcset) return;
      const rewritten = srcset
        .split(',')
        .map((part) => {
          const bits = part.trim().split(/\s+/);
          if (!bits[0]) return part.trim();
          if (
            /^(data:|blob:|https?:|\/\/)/i.test(bits[0]) ||
            bits[0].startsWith('#')
          ) {
            return part.trim();
          }
          bits[0] = resourceUrl(dictId, bits[0]);
          return bits.join(' ');
        })
        .join(', ');
      node.setAttribute('srcset', rewritten);
    });

    doc.querySelectorAll('[style]').forEach((node) => {
      node.setAttribute(
        'style',
        rewriteCssUrls(node.getAttribute('style') || '', dictId)
      );
    });

    doc.querySelectorAll('[background]').forEach((node) => {
      node.setAttribute(
        'background',
        resourceUrl(dictId, node.getAttribute('background') || '')
      );
    });
  }

  /* ============================== audio ============================== */

  async function playSound(dictId, rawPath) {
    const cleaned = stripControls(rawPath).replace(/^sound:\/\//i, '');
    if (!cleaned) return;
    const candidates = [cleaned];
    if (!/\.(wav|mp3|ogg|spx|m4a|aac|flac)$/i.test(cleaned)) {
      candidates.push(`${cleaned}.wav`, `${cleaned}.mp3`, `${cleaned}.spx`, `${cleaned}.ogg`);
    }

    try {
      currentAudio?.pause();
    } catch {
      /* ignore */
    }

    for (const candidate of candidates) {
      const url = resourceUrl(dictId, candidate);
      if (!url) continue;
      try {
        const response = await fetch(url, { cache: 'force-cache' });
        if (!response.ok) continue;
        const blob = await response.blob();
        if (await playBlob(blob)) return;
      } catch {
        /* try the next candidate */
      }
    }

    // The webview may not allow fetch on custom schemes; try direct playback.
    const directUrl = resourceUrl(dictId, candidates[0]);
    if (directUrl) {
      try {
        const audio = new Audio(directUrl);
        currentAudio = audio;
        await audio.play();
        return;
      } catch {
        /* fall through */
      }
    }
    toast('没有找到可播放的发音文件', 'warn', 2600);
  }

  function playBlob(blob) {
    return new Promise((resolve) => {
      const url = URL.createObjectURL(blob);
      const audio = new Audio(url);
      currentAudio = audio;
      let settled = false;
      const finish = (ok) => {
        if (settled) return;
        settled = true;
        if (!ok) URL.revokeObjectURL(url);
        resolve(ok);
      };
      audio.addEventListener('ended', () => URL.revokeObjectURL(url), { once: true });
      audio.addEventListener('error', () => finish(false), { once: true });
      audio.play().then(() => finish(true)).catch(() => finish(false));
    });
  }

  /* ============================== menus / modals / toasts ============================== */

  function closePopovers() {
    el.addPopover.hidden = true;
    hideSuggestions();
  }

  function openMenu(anchor, items) {
    closePopovers();
    el.menuLayer.hidden = false;
    el.menuLayer.replaceChildren();
    const menu = document.createElement('div');
    menu.className = 'menu';
    menu.setAttribute('role', 'menu');

    for (const item of items) {
      if (item.separator) {
        const sep = document.createElement('div');
        sep.className = 'menu-sep';
        menu.append(sep);
        continue;
      }
      const button = document.createElement('button');
      button.type = 'button';
      button.disabled = Boolean(item.disabled);
      if (item.danger) button.classList.add('danger');
      button.innerHTML = `${item.icon ? icon(item.icon) : ''}<span></span>`;
      button.querySelector('span').textContent = item.label;
      button.addEventListener('click', () => {
        closeMenu();
        item.action?.();
      });
      menu.append(button);
    }

    el.menuLayer.append(menu);
    const rect = menu.getBoundingClientRect();
    let left = anchor.left;
    let top = anchor.bottom + 6;
    if (left + rect.width > window.innerWidth - 8) {
      left = window.innerWidth - rect.width - 8;
    }
    if (top + rect.height > window.innerHeight - 8) {
      top = anchor.top - rect.height - 6;
    }
    menu.style.left = `${Math.max(8, left)}px`;
    menu.style.top = `${Math.max(8, top)}px`;
  }

  function closeMenu() {
    el.menuLayer.hidden = true;
    el.menuLayer.replaceChildren();
  }

  function openDictMenu(id, anchor) {
    const dict = state.dicts.find((item) => item.id === id);
    if (!dict) return;
    const index = state.dicts.indexOf(dict);
    const live = state.indexing.has(dict.id);
    openMenu(anchor, [
      { label: '重命名…', icon: 'book', action: () => renameDialog(dict) },
      { label: '设置标签…', icon: 'filter', action: () => tagsDialog(dict) },
      live
        ? {
            label: '取消索引',
            icon: 'x',
            action: () => invoke('cancel_index', { id: dict.id }),
          }
        : {
            label: '重建索引',
            icon: 'refresh',
            action: () => reindexDict(dict),
          },
      { separator: true },
      {
        label: '置顶',
        icon: 'top',
        disabled: index <= 0,
        action: () => moveDictTo(dict.id, 0),
      },
      {
        label: '上移',
        icon: 'up',
        disabled: index <= 0,
        action: () => moveDict(dict.id, -1),
      },
      {
        label: '下移',
        icon: 'down',
        disabled: index >= state.dicts.length - 1,
        action: () => moveDict(dict.id, 1),
      },
      {
        label: '置底',
        icon: 'bottom',
        disabled: index >= state.dicts.length - 1,
        action: () => moveDictTo(dict.id, state.dicts.length - 1),
      },
      { separator: true },
      {
        label: '打开所在文件夹',
        icon: 'folder',
        action: () =>
          invoke('open_dictionary_folder', { id: dict.id }).catch((error) =>
            toast(`无法打开文件夹：${error}`, 'error')
          ),
      },
      {
        label: '删除词典…',
        icon: 'trash',
        danger: true,
        action: () => removeDialog(dict),
      },
    ]);
  }

  async function reindexDict(dict) {
    if (state.indexing.has(dict.id)) {
      toast('这本词典正在索引中', 'info', 1800);
      return;
    }
    try {
      await invoke('reindex_dictionary', { id: dict.id });
      toast(`已将《${dict.name}》加入索引队列`, 'success', 2200);
    } catch (error) {
      toast(`无法开始索引：${error}`, 'error');
    }
  }

  function openModal(title, build) {
    closeModal();
    el.modalLayer.hidden = false;
    const modal = document.createElement('div');
    modal.className = 'modal';
    modal.setAttribute('role', 'dialog');
    modal.setAttribute('aria-modal', 'true');

    const head = document.createElement('div');
    head.className = 'modal-head';
    const heading = document.createElement('h2');
    heading.textContent = title;
    const closeBtn = document.createElement('button');
    closeBtn.className = 'icon-btn';
    closeBtn.title = '关闭';
    closeBtn.innerHTML = icon('x');
    closeBtn.addEventListener('click', closeModal);
    head.append(heading, closeBtn);

    const body = document.createElement('div');
    body.className = 'modal-body';
    const actions = document.createElement('div');
    actions.className = 'modal-actions';
    const spacer = document.createElement('div');
    spacer.className = 'spacer';
    actions.append(spacer);

    modal.append(head, body, actions);
    el.modalLayer.append(modal);
    build({ body, actions, close: closeModal, modal });

    modalKeyHandler = (event) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        closeModal();
      }
    };
    document.addEventListener('keydown', modalKeyHandler, true);
    requestAnimationFrame(() => {
      const focusable = body.querySelector('input, select, button');
      focusable?.focus();
    });
  }

  function closeModal() {
    if (modalKeyHandler) {
      document.removeEventListener('keydown', modalKeyHandler, true);
      modalKeyHandler = null;
    }
    el.modalLayer.hidden = true;
    el.modalLayer.replaceChildren();
  }

  function modalButton(label, cls, onClick) {
    const button = document.createElement('button');
    button.className = `btn ${cls || ''}`.trim();
    button.textContent = label;
    button.addEventListener('click', onClick);
    return button;
  }

  function renameDialog(dict) {
    openModal('重命名词典', ({ body, actions, close }) => {
      body.innerHTML = `
        <div class="field">
          <label for="rename-input">显示名称</label>
          <input id="rename-input" type="text" maxlength="80" autocomplete="off">
        </div>
        <p class="modal-note">只影响列表中的显示名称，不会修改磁盘上的文件。</p>
      `;
      const input = body.querySelector('#rename-input');
      input.value = dict.name || '';
      actions.append(
        modalButton('取消', 'btn-ghost', close),
        modalButton('保存', 'btn-primary', async () => {
          const name = input.value.trim();
          if (!name) {
            input.focus();
            return;
          }
          try {
            await invoke('rename_dictionary', { id: dict.id, name });
            close();
            await refreshDicts();
          } catch (error) {
            toast(`重命名失败：${error}`, 'error');
          }
        })
      );
      setTimeout(() => {
        input.focus();
        input.select();
      }, 30);
    });
  }

  function tagsDialog(dict) {
    openModal('设置标签', ({ body, actions, close }) => {
      body.innerHTML = `
        <div class="field">
          <label for="tags-input">标签（用逗号分隔，最多 8 个）</label>
          <input id="tags-input" type="text" maxlength="200" autocomplete="off">
        </div>
        <p class="modal-note">标签用于侧栏快速筛选，例如「英汉」「牛津」「有声」。</p>
      `;
      const input = body.querySelector('#tags-input');
      input.value = (dict.tags || []).join(', ');
      actions.append(
        modalButton('取消', 'btn-ghost', close),
        modalButton('保存', 'btn-primary', async () => {
          const tags = input.value
            .split(/[,，]/)
            .map((tag) => tag.trim())
            .filter(Boolean);
          try {
            await invoke('set_dictionary_tags', { id: dict.id, tags });
            close();
            await refreshDicts();
            toast('标签已保存', 'success', 1800);
          } catch (error) {
            toast(`保存标签失败：${error}`, 'error');
          }
        })
      );
    });
  }

  function tagsManagerDialog() {
    openModal('标签管理', ({ body, actions, close }) => {
      body.innerHTML = `
        <p class="modal-note">用逗号分隔多个标签，最多 8 个。保存后可在侧栏按标签筛选。</p>
        <div class="tag-manage-list"></div>
      `;
      const list = body.querySelector('.tag-manage-list');
      for (const dict of state.dicts) {
        const row = document.createElement('div');
        row.className = 'tag-manage-row';
        row.dataset.id = dict.id;
        const label = document.createElement('span');
        label.textContent = dict.name || '未命名词典';
        label.title = dict.path || '';
        const input = document.createElement('input');
        input.type = 'text';
        input.maxLength = 200;
        input.value = (dict.tags || []).join(', ');
        row.append(label, input);
        list.append(row);
      }
      actions.append(
        modalButton('取消', 'btn-ghost', close),
        modalButton('保存标签', 'btn-primary', async () => {
          const rows = [...list.querySelectorAll('.tag-manage-row')];
          try {
            for (const row of rows) {
              const tags = row
                .querySelector('input')
                .value.split(/[,，]/)
                .map((tag) => tag.trim())
                .filter(Boolean);
              await invoke('set_dictionary_tags', { id: row.dataset.id, tags });
            }
            close();
            await refreshDicts();
            toast('标签已保存', 'success', 1800);
          } catch (error) {
            toast(`保存标签失败：${error}`, 'error');
          }
        })
      );
    });
  }

  function removeDialog(dict) {
    openModal('删除词典', ({ body, actions, close }) => {
      body.innerHTML = `
        <p class="modal-note">将从列表移除 <strong>${escapeHtml(dict.name)}</strong>，并删除本地索引缓存。</p>
        <p class="modal-note">磁盘上的 .mdx 文件不会被删除，以后可以再次添加。</p>
      `;
      actions.append(
        modalButton('取消', 'btn-ghost', close),
        modalButton('删除', 'btn-danger', async () => {
          try {
            await invoke('remove_dictionary', { id: dict.id });
            close();
            await refreshDicts();
            toast('词典已移除', 'success', 1800);
          } catch (error) {
            toast(`删除失败：${error}`, 'error');
          }
        })
      );
    });
  }

  function settingsDialog() {
    openModal('设置', ({ body, actions, close }) => {
      const settings = state.settings;
      let themeChoice = settings.theme || 'system';
      let contentChoice = state.contentTheme || 'alwaysLight';
      const info = state.appInfo || {};
      body.innerHTML = `
        <div class="field">
          <span class="field-label">外观</span>
          <div class="segmented" id="theme-seg">
            <button data-theme-choice="system">跟随系统</button>
            <button data-theme-choice="light">浅色</button>
            <button data-theme-choice="dark">深色</button>
          </div>
        </div>
        <div class="field">
          <span class="field-label">释义内容主题</span>
          <div class="segmented" id="content-theme-seg">
            <button data-content-theme="alwaysLight">始终浅色</button>
            <button data-content-theme="followApp">跟随外壳</button>
            <button data-content-theme="alwaysDark">始终深色</button>
          </div>
          <p class="modal-note">默认「始终浅色」保持原有纸面样式；深色仅作用于释义内容。</p>
        </div>
        <div class="field">
          <span class="field-label">释义字号</span>
          <div class="field-row">
            <input id="font-range" type="range" min="12" max="24" step="1" value="${Number(settings.fontSize) || 16}">
            <span class="range-value" id="font-value">${Number(settings.fontSize) || 16}px</span>
          </div>
        </div>
        <div class="field">
          <span class="field-label">输入联想条数</span>
          <div class="field-row">
            <input id="suggest-range" type="range" min="5" max="30" step="1" value="${Number(settings.suggestionLimit) || 12}">
            <span class="range-value" id="suggest-value">${Number(settings.suggestionLimit) || 12}</span>
          </div>
        </div>
        <div class="field">
          <span class="field-label">默认展开结果数</span>
          <div class="field-row">
            <input id="expand-range" type="range" min="0" max="10" step="1" value="${Math.max(0, Number(settings.defaultExpanded ?? 3))}">
            <span class="range-value" id="expand-value">${formatExpanded(settings.defaultExpanded ?? 3)}</span>
          </div>
        </div>
        <div class="field">
          <div class="switch-row">
            <div class="switch-text"><strong>启动时自动建立索引</strong><span>词典文件更新后自动重建</span></div>
            <input class="switch" id="auto-index" type="checkbox" ${settings.autoIndex ? 'checked' : ''}>
          </div>
          <div class="switch-row">
            <div class="switch-text"><strong>显示未收录的词典</strong><span>结果中列出没有命中词条的词典</span></div>
            <input class="switch" id="show-all" type="checkbox" ${settings.showAllDicts ? 'checked' : ''}>
          </div>
        </div>
        <div class="section-title">数据位置</div>
        <div class="paths">
          <div class="path-row"><span>配置</span><code>${escapeHtml(info.configPath || '—')}</code></div>
          <div class="path-row"><span>索引</span><code>${escapeHtml(info.indexDir || '—')}</code></div>
          <div class="path-row"><span>日志</span><code>${escapeHtml(info.logDir || '—')}</code></div>
          <div class="path-row"><span>版本</span><code>${escapeHtml(info.version || '0.1.0')} · ${escapeHtml(info.platform || '')}</code></div>
        </div>
      `;

      const themeSeg = body.querySelector('#theme-seg');
      const paintTheme = () => {
        themeSeg.querySelectorAll('button').forEach((button) => {
          button.classList.toggle('active', button.dataset.themeChoice === themeChoice);
        });
      };
      paintTheme();
      themeSeg.addEventListener('click', (event) => {
        const button = event.target.closest('[data-theme-choice]');
        if (!button) return;
        themeChoice = button.dataset.themeChoice;
        paintTheme();
      });

      const contentSeg = body.querySelector('#content-theme-seg');
      const paintContent = () => {
        contentSeg.querySelectorAll('button').forEach((button) => {
          button.classList.toggle('active', button.dataset.contentTheme === contentChoice);
        });
      };
      paintContent();
      contentSeg.addEventListener('click', (event) => {
        const button = event.target.closest('[data-content-theme]');
        if (!button) return;
        contentChoice = button.dataset.contentTheme;
        paintContent();
      });

      const fontRange = body.querySelector('#font-range');
      const fontValue = body.querySelector('#font-value');
      fontRange.addEventListener('input', () => {
        fontValue.textContent = `${fontRange.value}px`;
      });

      const suggestRange = body.querySelector('#suggest-range');
      const suggestValue = body.querySelector('#suggest-value');
      suggestRange.addEventListener('input', () => {
        suggestValue.textContent = suggestRange.value;
      });

      const expandRange = body.querySelector('#expand-range');
      const expandValue = body.querySelector('#expand-value');
      expandRange.addEventListener('input', () => {
        expandValue.textContent = formatExpanded(expandRange.value);
      });

      const autoIndex = body.querySelector('#auto-index');
      const showAll = body.querySelector('#show-all');

      actions.append(
        modalButton('全部重建索引', 'btn-ghost', async () => {
          try {
            await invoke('reindex_all');
            toast('已将所有词典加入索引队列', 'success', 2200);
          } catch (error) {
            toast(`无法开始重建：${error}`, 'error');
          }
        }),
        modalButton('打开数据目录', 'btn-ghost', () => {
          invoke('open_data_dir').catch((error) =>
            toast(`无法打开目录：${error}`, 'error')
          );
        }),
        modalButton('标签管理…', 'btn-ghost', tagsManagerDialog),
        modalButton('取消', 'btn-ghost', close),
        modalButton('保存设置', 'btn-primary', async () => {
          const next = {
            theme: themeChoice,
            fontSize: Number(fontRange.value) || 16,
            suggestionLimit: Number(suggestRange.value) || 12,
            defaultExpanded: Number(expandRange.value) || 0,
            autoIndex: autoIndex.checked,
            showAllDicts: showAll.checked,
          };
          try {
            state.settings = { ...state.settings, ...(await invoke('set_settings', { settings: next })) };
            saveContentTheme(contentChoice);
            applyTheme();
            close();
            toast('设置已保存', 'success', 1800);
            if (state.query) runSearch(state.query);
          } catch (error) {
            toast(`保存设置失败：${error}`, 'error');
          }
        })
      );
    });
  }

  function toast(message, type = 'info', timeout = 3600) {
    const node = document.createElement('div');
    node.className = `toast ${type}`;
    const iconName =
      type === 'success' ? 'check' : type === 'error' || type === 'warn' ? 'alert' : 'search';
    node.innerHTML = `${icon(iconName)}<div class="toast-text"></div>`;
    node.querySelector('.toast-text').textContent = String(message);
    el.toasts.append(node);
    setTimeout(() => {
      node.classList.add('leaving');
      setTimeout(() => node.remove(), 240);
    }, timeout);
  }

  function showFatal(error) {
    document.body.innerHTML = '';
    const node = document.createElement('div');
    node.className = 'fatal';
    node.textContent = `MoeDict 启动失败\n\n${error?.stack || error}\n\n请查看日志或重新启动应用。`;
    document.body.append(node);
  }

  function showDropOverlay(visible) {
    el.dropOverlay.hidden = !visible;
  }

  /* ============================== events ============================== */

  function bindEvents() {
    el.searchInput.addEventListener('input', onSearchInput);
    el.searchInput.addEventListener('keydown', onSearchKeydown);
    el.searchInput.addEventListener('focus', () => {
      if (el.searchInput.value.trim() && el.suggestions.hidden) {
        fetchSuggestions(el.searchInput.value.trim());
      }
    });

    el.clearBtn.addEventListener('click', resetToWelcome);

    el.navToggle.addEventListener('click', () => setNavPane(!state.navPaneOpen));
    el.navPaneClose.addEventListener('click', () => setNavPane(false));
    el.sidebarToggle.addEventListener('click', () => setSidebar(!state.sidebarOpen));
    el.sidebarReveal.addEventListener('click', () => setSidebar(true));
    el.historyBack.addEventListener('click', () => navigateHistory(-1));
    el.historyForward.addEventListener('click', () => navigateHistory(1));
    el.settingsBtn.addEventListener('click', settingsDialog);

    el.results.addEventListener('scroll', scheduleNavUpdate, { passive: true });
    for (const type of ['wheel', 'touchstart', 'pointerdown', 'keydown']) {
      el.results.addEventListener(type, cancelResultsScroll, { passive: true });
    }

    el.addBtn.addEventListener('click', (event) => {
      event.stopPropagation();
      const willOpen = el.addPopover.hidden;
      closePopovers();
      if (willOpen) el.addPopover.hidden = false;
    });

    el.dictFilter.addEventListener('input', () => {
      state.filter = el.dictFilter.value;
      renderDicts();
    });

    el.dictTags.addEventListener('click', (event) => {
      const chip = event.target.closest('[data-tag]');
      if (!chip) return;
      const next = chip.dataset.tag || '';
      if (next === state.tagFilter) return;
      state.tagFilter = next;
      renderDicts();
      if (state.query) runSearch(state.query);
      else if (el.searchInput.value.trim()) fetchSuggestions(el.searchInput.value.trim());
    });

    el.dictList.addEventListener('change', (event) => {
      const checkbox = event.target.closest('input[type="checkbox"]');
      if (!checkbox) return;
      const row = checkbox.closest('.dict-row');
      if (!row) return;
      toggleDict(row.dataset.id, checkbox.checked);
    });

    el.dictList.addEventListener('click', (event) => {
      const row = event.target.closest('.dict-row');
      if (!row) return;
      if (event.target.closest('[data-role="more"]')) {
        event.stopPropagation();
        openDictMenu(row.dataset.id, event.target.closest('[data-role="more"]').getBoundingClientRect());
        return;
      }
      if (event.target.closest('input, button, .dict-drag')) return;
      focusDictionary(row.dataset.id);
    });

    el.dictList.addEventListener('keydown', (event) => {
      const row = event.target.closest('.dict-row');
      if (!row) return;
      const key = (event.key || '').toLowerCase();
      if (key === 'enter') {
        event.preventDefault();
        focusDictionary(row.dataset.id);
        return;
      }
      if ((event.ctrlKey || event.altKey) && (key === 'arrowup' || key === 'arrowdown')) {
        event.preventDefault();
        if (isDictFiltered()) {
          toast('清除筛选后可调整顺序', 'info', 1800);
          return;
        }
        const id = row.dataset.id;
        moveDict(id, key === 'arrowup' ? -1 : 1);
        const position = state.dicts.findIndex((dict) => dict.id === id);
        if (position >= 0) toast(`已移动到第 ${position + 1} 位`, 'success', 1600);
        const moved = $$('.dict-row', el.dictList).find((item) => item.dataset.id === id);
        moved?.focus();
      }
    });

    el.dictList.addEventListener('dragstart', (event) => {
      const handle = event.target.closest('[data-role="drag"]');
      const row = event.target.closest('.dict-row');
      if (!handle || !row || !handle.draggable) {
        event.preventDefault();
        return;
      }
      state.dragId = row.dataset.id;
      event.dataTransfer.effectAllowed = 'move';
      try {
        event.dataTransfer.setData('text/plain', state.dragId);
      } catch {
        /* ignore */
      }
      requestAnimationFrame(() => row.classList.add('dragging'));
    });

    el.dictList.addEventListener('dragover', (event) => {
      if (!state.dragId) return;
      event.preventDefault();
      const row = event.target.closest('.dict-row');
      clearDropMarks();
      if (row && row.dataset.id !== state.dragId) {
        const rect = row.getBoundingClientRect();
        const after = event.clientY > rect.top + rect.height / 2;
        row.classList.add(after ? 'drop-after' : 'drop-before');
      }
      event.dataTransfer.dropEffect = 'move';
    });

    el.dictList.addEventListener('dragleave', (event) => {
      if (!el.dictList.contains(event.relatedTarget)) clearDropMarks();
    });

    el.dictList.addEventListener('drop', (event) => {
      if (!state.dragId) return;
      event.preventDefault();
      const row = event.target.closest('.dict-row');
      const after = row ? row.classList.contains('drop-after') : true;
      applyDrag(state.dragId, row?.dataset.id, after);
      clearDropMarks();
      state.dragId = null;
    });

    el.dictList.addEventListener('dragend', () => {
      $$('.dict-row.dragging', el.dictList).forEach((row) => row.classList.remove('dragging'));
      clearDropMarks();
      state.dragId = null;
    });

    el.suggestions.addEventListener('mousedown', (event) => {
      const option = event.target.closest('.suggestion');
      if (!option) return;
      event.preventDefault();
      runSearch(option.dataset.word);
    });

    el.menuLayer.addEventListener('mousedown', (event) => {
      if (event.target === el.menuLayer) closeMenu();
    });

    el.modalLayer.addEventListener('mousedown', (event) => {
      if (event.target === el.modalLayer) closeModal();
    });

    document.addEventListener('click', (event) => {
      const action = event.target.closest('[data-action]')?.dataset.action;
      if (!action) return;
      if (action === 'pick-files') pickFiles();
      else if (action === 'pick-folder') pickFolder();
      else if (action === 'lucky') runLucky();
      else if (action === 'settings') settingsDialog();
      else if (action === 'enable-all') enableAll();
    });

    document.addEventListener('pointerdown', (event) => {
      if (
        !el.addPopover.hidden &&
        !el.addPopover.contains(event.target) &&
        !el.addBtn.contains(event.target)
      ) {
        el.addPopover.hidden = true;
      }
      if (!el.searchShell.contains(event.target)) hideSuggestions();
    });

    el.resultsList.addEventListener('click', (event) => {
      const card = event.target.closest('.result-card');
      if (!card) return;
      const action = event.target.closest('[data-action]')?.dataset.action;
      if (action === 'copy') {
        event.preventDefault();
        copyCard(card);
        return;
      }
      if (action === 'collapse') {
        event.preventDefault();
        toggleCollapse(card);
        return;
      }
      if (event.target.closest('.result-head') && !event.target.closest('button')) {
        toggleCollapse(card);
      }
    });

    el.resultsMeta.addEventListener('click', (event) => {
      const action = event.target.closest('[data-action]')?.dataset.action;
      if (action === 'expand-all') {
        $$('.result-card', el.resultsList).forEach((card) => {
          card.classList.remove('collapsed');
          card.querySelector('[data-action="collapse"]')?.setAttribute('aria-expanded', 'true');
          ensureFrame(card);
        });
      } else if (action === 'collapse-all') {
        $$('.result-card', el.resultsList).forEach((card) => {
          card.classList.add('collapsed');
          card.querySelector('[data-action="collapse"]')?.setAttribute('aria-expanded', 'false');
        });
      }
    });

    el.clearHistoryBtn.addEventListener('click', clearHistory);

    const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
    $$('.shortcut').forEach((node) => {
      node.textContent = isMac ? '⌘ L' : 'Ctrl L';
    });

    document.addEventListener('keydown', onGlobalKeydown);

    darkQuery.addEventListener?.('change', () => {
      if ((state.settings.theme || 'system') === 'system') applyTheme();
    });

    window.addEventListener('dragover', (event) => {
      if (event.dataTransfer?.types?.includes('Files')) {
        event.preventDefault();
        showDropOverlay(true);
      }
    });
    window.addEventListener('dragleave', (event) => {
      if (event.relatedTarget === null) showDropOverlay(false);
    });
    window.addEventListener('drop', (event) => {
      if (!event.dataTransfer?.files?.length) return;
      event.preventDefault();
      showDropOverlay(false);
      const paths = Array.from(event.dataTransfer.files)
        .map((file) => file.path)
        .filter(Boolean);
      if (paths.length) addPaths(paths);
      else toast('请使用“添加 MDX 文件”按钮选择词典', 'warn', 3000);
    });

    window.addEventListener('unhandledrejection', (event) => {
      console.error('unhandled rejection', event.reason);
    });
  }

  function clearDropMarks() {
    $$('.dict-row.drop-before, .dict-row.drop-after', el.dictList).forEach((row) => {
      row.classList.remove('drop-before', 'drop-after');
    });
  }

  function applyDrag(id, targetId, after) {
    if (!targetId || id === targetId) return;
    const items = state.dicts.slice();
    const from = items.findIndex((dict) => dict.id === id);
    if (from < 0) return;
    const [item] = items.splice(from, 1);
    let to = items.findIndex((dict) => dict.id === targetId);
    if (to < 0) to = items.length;
    else if (after) to += 1;
    items.splice(to, 0, item);
    state.dicts = items;
    renderDicts();
    persistOrder(items.map((dict) => dict.id));
  }

  function onGlobalKeydown(event) {
    const mod = event.metaKey || event.ctrlKey;
    const key = (event.key || '').toLowerCase();

    if (!el.modalLayer.hidden) return;

    if (mod && event.shiftKey && key === 'n') {
      event.preventDefault();
      setNavPane(!state.navPaneOpen);
      return;
    }
    if (mod && !event.shiftKey && !event.altKey && key === 'b') {
      event.preventDefault();
      setSidebar(!state.sidebarOpen);
      return;
    }
    if (mod && !event.altKey && !event.shiftKey && (key === 'l' || key === 'k')) {
      event.preventDefault();
      focusSearch();
      return;
    }
    if (event.altKey && (key === 'arrowleft' || key === 'arrowright')) {
      event.preventDefault();
      navigateHistory(key === 'arrowleft' ? -1 : 1);
      return;
    }
    if (mod && key === ',') {
      event.preventDefault();
      settingsDialog();
      return;
    }
    if (mod && key === 'o') {
      event.preventDefault();
      if (event.shiftKey) pickFolder();
      else pickFiles();
      return;
    }
    if (key === 'escape') {
      if (!el.menuLayer.hidden) {
        closeMenu();
        return;
      }
      if (!el.addPopover.hidden) {
        el.addPopover.hidden = true;
        return;
      }
      if (!el.suggestions.hidden) {
        hideSuggestions();
        return;
      }
      if (el.searchInput.value) {
        resetToWelcome();
      }
      return;
    }
    if (key === '/' && !isEditable(event.target) && el.modalLayer.hidden) {
      event.preventDefault();
      focusSearch();
    }
  }

  function isEditable(node) {
    const element = node instanceof Element ? node : null;
    if (!element) return false;
    return Boolean(
      element.closest('input, textarea, select, [contenteditable="true"]')
    );
  }

  async function runLucky() {
    try {
      const word = await invoke('lucky', { tags: activeTags() });
      if (word) runSearch(word);
      else toast('没有可用的词典，请先添加并建立索引', 'warn', 3000);
    } catch (error) {
      toast(`随机查询失败：${error}`, 'error');
    }
  }

  async function setupTauriListeners() {
    if (!listen) return;
    await listen('index-progress', (event) => {
      const progress = event.payload;
      if (!progress?.id) return;
      state.indexing.set(progress.id, progress);
      updateDictRow(progress.id);
      updateStatus();
    });
    await listen('index-done', (event) => {
      const payload = event.payload || {};
      state.indexing.delete(payload.id);
      if (!payload.cancelled) {
        if (payload.ok) {
          toast(`《${payload.name || '词典'}》索引完成`, 'success', 2400);
        } else {
          toast(`《${payload.name || '词典'}》索引失败：${payload.error || '未知错误'}`, 'error', 6000);
        }
      }
      updateStatus();
      scheduleRefresh();
    });
    await listen('dict-changed', () => scheduleRefresh());

    const dragEvents = [
      ['tauri://drag-enter', () => showDropOverlay(true)],
      ['tauri://drag-over', () => showDropOverlay(true)],
      ['tauri://drag-leave', () => showDropOverlay(false)],
      [
        'tauri://drag-drop',
        (event) => {
          showDropOverlay(false);
          const paths = event.payload?.paths;
          if (Array.isArray(paths) && paths.length) addPaths(paths);
        },
      ],
    ];
    for (const [name, handler] of dragEvents) {
      try {
        await listen(name, handler);
      } catch {
        /* older Tauri versions may use different event names */
      }
    }
  }

  async function init() {
    bindEvents();
    state.history = loadHistory();
    state.contentTheme = loadContentTheme();
    state.navPaneOpen = loadNavPane();
    state.sidebarOpen = loadSidebar();
    applyTheme();
    setNavPane(state.navPaneOpen);
    setSidebar(state.sidebarOpen);
    updateHistoryButtons();
    try {
      state.appInfo = await invoke('app_info');
      state.resourceBase = state.appInfo?.resourceBase || state.resourceBase;
      const settings = await invoke('get_settings');
      state.settings = { ...DEFAULTS, ...(settings || {}) };
      applyTheme();
      await setupTauriListeners();
      await refreshDicts();
      renderRecent();
      updateStatus();
      // Pick up external edits to dictionary files without a filesystem watcher.
      setInterval(() => {
        if (
          !document.hidden &&
          !state.dragId &&
          el.modalLayer.hidden &&
          el.menuLayer.hidden
        ) {
          scheduleRefresh();
        }
      }, 60_000);
      if (state.dicts.length) el.searchInput.focus();
    } catch (error) {
      showFatal(error);
    }
  }

  init();
})();
