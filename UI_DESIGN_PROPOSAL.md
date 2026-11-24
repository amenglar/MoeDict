# MoeDict UI/UX 设计方案

> 状态：调研与方案（未改动代码）
> 范围：查询结果的导航与跳转、词典顺序管理、性能优化、暗色主题下的词典内容配色，以及整体易用性与现代化设计。

本文档基于两部分事实：

1. **代码盘点**：`ui/` 前端全量（HTML/CSS/JS）、`src-tauri` 与 `crates/mdict-core` 的 IPC/核心能力。
2. **主流词典调研**：GoldenDict(-ng)、MDict、欧路 Eudic、DictTango、Apple 词典，以及通用 UX / 暗色模式实现范式。

---

## 1. 现状盘点

### 1.1 布局与主题

- 布局（`ui/index.html`）：`aside#sidebar`（固定 292px 词典列表）＋ `main`（顶栏搜索 / 结果区 / 状态栏）。仅 `.results` 一个滚动区，全局只有一个 980px 断点，桌面优先。
- 动态 UI（词典行、结果卡、菜单、模态框、toast）全部由 `app.js` 的字符串模板 + `createElement` 生成，`index.html` 是稳定外壳。
- 配色系统是 CSS 变量驱动（`styles.css` 的 `:root` 与 `html[data-theme="dark"]`），使用大量 `color-mix()`；设计基调为「warm paper, ink and terracotta」。
- 外壳主题：`applyTheme()`（`app.js:209`）解析 `system | light | dark` 并写到 `document.documentElement.dataset.theme`。设置存于 Rust `config.json`，**不是 localStorage**。

### 1.2 关键痛点

| 领域 | 现状 | 问题 |
|---|---|---|
| 释义内容配色 | `baseCss()`（`app.js:186-203`）硬编码 `background:#fff; color:#1d2330`；`.result-body/.entry-frame/展开全文/骨架屏` 也写死白色系（`styles.css:1070-1144`） | 暗色模式下外壳变黑，但每张释义卡仍是「白纸」，割裂刺眼 |
| 结果导航 | 每本命中词典一张卡片按 order 渲染（`renderResults` `app.js:756`），仅有 `focusDictionary()` 滚动定位（`app.js:1133`） | 无词典锚点导航、无词条上一条/下一条、无查询前进/后退 |
| 顺序管理 | 拖拽（`applyDrag` `app.js:2012`）＋右键「上移/下移」；拖拽纯鼠标且**筛选时禁用**（`dictRow` `app.js:308`） | 无键盘重排；筛选时禁用拖拽易导致顺序错乱；无分组 |
| 性能 | 已做懒加载、懒高度、防抖、序号守卫、60s 轮询 | 多词典时并存大量活 iframe；重复查询重复清洗 DOM；无渲染缓存 |
| 能力边界 | IPC 仅 20 个命令（`commands.rs`） | 无收藏/生词本、无全文/模糊搜索、无词条相邻导航、无词典描述字段；历史仅前端 localStorage |

### 1.3 现有基础（应保留）

- IntersectionObserver 懒加载 iframe（`rootMargin:'420px'`）、ResizeObserver 自适应高度、>1500px 的「展开全文」。
- 搜索/联想 140ms 防抖 + `querySeq/suggestSeq` 序号守卫（防止旧响应覆盖新结果）。
- iframe `sandbox="allow-same-origin"`（禁脚本但可读 DOM 做尺寸/复制/事件委托）。
- 索引进度后端 80ms 节流（`indexer.rs:144`）、事件 `index-progress / index-done / dict-changed`。
- 无障碍基础：语义化 role、aria-label、`aria-live` 的 toast、平台自适应快捷键、`prefers-reduced-motion`。

---

## 2. 主流词典调研

### 2.1 功能对照

| 功能 | GoldenDict(-ng) | MDict | 欧路 Eudic | DictTango | 本项目现状 |
|---|---|---|---|---|---|
| 多词典联合查询/排序 | ✅ 强，支持**词典分组** | ✅ 可选单本/全部 | ⚠️ 免费版限 3 本 | ✅ 分组＋联合/瀑布流/滑动视图 | ✅ 按 order |
| **结果导航窗格** | ✅ 右侧 Results Navigation Pane，点击词典名**即时跳转**；右键分组菜单跳转 | ⚠️ 靠滚动 | ❌ | ✅ 阅读模式 | ❌ 仅滚动定位 |
| **词条上一条/下一条** | 仅历史前进/后退 | ✅ **4 个箭头**按词条顺序翻页＋历史 | 历史前进/后退 | ✅ | ❌ |
| 查询历史 | ✅ | ✅ | ✅ | ✅ | ✅（仅欢迎页 chips） |
| 收藏/生词本/笔记/高亮 | ❌ | ❌ | ✅ 强（同步、复习） | ✅ 批注＋单词本 | ❌ |
| 屏幕/划词/剪贴板取词 | ✅ | 仅剪贴板 | ✅ 三者 | ✅ | ❌（仅双击选词查词） |
| 全文搜索 | ✅（但卡） | ❌ | ⚠️ | ✅ | ❌ |
| 字体/排版控制 | 字号 | 字号＋**字体** | 字号 | **全局字体＋每词典字体** | 字号 12–24 |
| 现代化界面 | 中 | 差（IE 内核） | 好 | 好 | 中（warm paper 风格） |

### 2.2 可借鉴要点

- **GoldenDict**：右侧「结果导航窗格」+分组点击跳转，是本项目的首要借鉴对象。
- **MDict**：按词条顺序的上一条/下一条箭头翻页，补齐「顺序浏览」能力。
- **欧路 / DictTango**：生词本、复习、批注、阅读模式——学习类增强（P1/P2）。
- **通用 UX 原则**（Material / Figma / Apple HIG）：渐进式披露、沿用熟悉范式、视觉层级、每个操作有反馈、WCAG AA 对比度、避免深层导航、键盘可达。

### 2.3 暗色模式实现范式调研

| 方案 | 做法 | 优点 | 缺点 | 结论 |
|---|---|---|---|---|
| CSS 变量双主题 | `:root` 与 `[data-theme=dark]` 各一套变量 | 正规、可控 | 维护两套样式 | 外壳已在用；内容层同样适用 |
| **iframe 内生成暗色基础样式** | 我们自建 `srcdoc`，按配置生成 `baseCss()` 变量层 | 最干净，不反相图片/emoji | 需处理词典自带硬编码浅色 CSS | **推荐主方案** |
| CSS 滤镜 `invert(1) hue-rotate(180deg)` | 一行反相 | 极简 | 反相图片/图标/emoji、去饱和、媒体需二次复原 | 仅作兜底「强制深色」 |
| 选择性反相 `data-invert-on-dark` | 仅对深字透明底图片反相 | 精准 | 需逐图标注 | 可选增强项 |

---

## 3. 设计原则

1. **渐进式披露**：高频操作（搜索/跳转/收藏）留在主界面；低频管理（顺序、字体、分组）收进设置/专用面板。
2. **保持惯例**：顶部搜索、左侧列表、点击词典名跳转——沿用现有肌肉记忆，不推翻外壳。
3. **反馈闭环**：重排、收藏、重建索引、跳转都有 toast/动效/aria 播报。
4. **无回归默认**：暗色内容、新外观默认保持「和现在一模一样」，用户主动开启才变化。
5. **键盘优先可达**：所有鼠标操作提供等价键盘路径。

---

## 4. 整体布局方案（渐进增强，保留现有外壳）

```
┌───────────┬────────────────────────────────────┬───────────────┐
│ 侧栏       │ 顶栏：搜索框 + 历史◀▶ + 操作按钮     │ 结果导航窗格   │
│ 词典列表   ├────────────────────────────────────┤ （可折叠，     │
│ (筛选/勾选)│ 结果区：词典跳转 chips 条             │  默认窄/隐藏） │
│           │  → 卡片1（词典A）                    │ A ● 3 条       │
│           │  → 卡片2（词典B）                    │ B ● 1 条       │
│           │  → …                                │ C ○ 未收录     │
│ 底部操作   │                                    │               │
├───────────┴────────────────────────────────────┴───────────────┤
│ 状态栏：N/M 启用 · 查询结果数/耗时 · 索引进度                    │
└────────────────────────────────────────────────────────────────┘
```

- 新增**可折叠右侧导航窗格**（借鉴 GoldenDict），由顶栏按钮或 `Ctrl+Shift+N` 切换；宽屏（≥1200px）默认显示、窄屏折叠为抽屉，避免挤压结果区。
- 顶栏搜索区增加**查询历史前进/后退 ◀▶**（纯前端历史栈）和**收藏星标**。
- 结果区顶部增加**词典跳转 chips 条**（「全部 5 · 牛津 1 · 朗文 1 …」），点击即滚动定位并 flash。
- `index.html` 外壳基本稳定，改动集中在 `app.js` 与 `styles.css`。

---

## 5. 查询结果的导航与跳转

| # | 功能 | 阶段 | 后端 | 说明 |
|---|---|---|---|---|
| 1 | 词典锚点导航 | P0 | 否 | 右侧窗格 chips + 结果区顶部 chips 两套入口，复用 `focusDictionary()` 的平滑滚动与 `.flash`；用 IntersectionObserver + `aria-current` 标记当前可视词典 |
| 2 | 查询前进/后退 | P0 | 否 | 把 `rememberSearch()` 的历史栈扩展为带游标的历史导航栈，`Alt+←/→`（Mac `⌘←/→`）切换，解决「只能点 chip、不能顺序回退」 |
| 3 | 卡片间键盘导航 | P0 | 否 | `J/K` 或 `Ctrl+↑/↓` 在命中卡片间移动焦点，`Enter` 折叠/展开，聚焦时 `focus-visible` 描边 |
| 4 | 词条上一条/下一条 | P1 | 是 | `IndexFile` 已有 `key(index)/definition(index)/random()`（`format.rs:126-253`）；新增 `adjacent_entry(dict_id, headword, delta)` 返回相邻词头，前端在卡片头提供 ◀/▶，补齐 MDict 式「按词条顺序翻页」 |
| 5 | 未收录词典区分 | P0 | 否 | 在导航窗格用空心点标记 `show_all_dicts` 下无命中的词典 |

---

## 6. 词典顺序管理

1. **筛选时也允许重排（P0）**：移除 `dictRow` 中 `row.draggable = !state.filter.trim()` 的限制——重排应作用于完整 order，而非过滤后的视图（当前逻辑易导致顺序错乱）。
2. **键盘重排（P0）**：行聚焦后 `Ctrl/Alt+↑/↓` 移动，toast 播报「已移动到第 N 位」，`aria-live` 同步。
3. **专用「词典管理」面板（P1）**：从侧栏底部/设置进入的全屏 modal，大行高拖拽、插入指示线、拖拽残影，`上移/下移/置顶/置底`，并支持排序依据切换（自定义 / 名称 / 词条数）；把低频复杂操作从主界面移走（渐进式披露）。
4. **词典分组（P1，需扩展 config）**：新增 `groups` 字段，支持整组启用/禁用与「按组跳转」，对应 GoldenDict/DictTango 的核心体验；`#[serde(default)]` 保证旧配置兼容。

---

## 7. 暗色主题下的词典内容配色

目标：**允许保留当前内容配色不变，同时单独引入一套控制「内容配色」的暗色主题配置。**

### 7.1 核心思路

释义 iframe 的 `srcdoc` 由我们自己生成（`buildDocument()` `app.js:1149`），因此最干净的方案不是 CSS `filter: invert()`，而是在 `baseCss()` 注入**内容主题变量层**，按配置生成不同基础样式——不反相图片/emoji、无饱和度损失。

### 7.2 新增独立设置组「释义外观 · 内容主题」

| 设置项 | 取值 / 默认 | 说明 |
|---|---|---|
| `contentTheme` | `alwaysLight`（默认）/ `followApp` / `alwaysDark` / `custom` | 默认 `alwaysLight` → **完全保留当前白色纸面，零回归** |
| `contentDarkBg / Text / Link / Border` | 预设 `#1c1f27 / #e9e5da / #f09472 / #2a2e38` | `custom` 时可改，对齐外壳 `--surface/--text/--accent-ink` |
| `invertMediaOnDark` | `false` | 仅对深字透明底的插图/PNG 选择性反相（`data-invert-on-dark`，参考 Dark Reader/Astro 思路），默认关 |

### 7.3 实现要点

- `baseCss(contentTheme)` 在 `html:root` 写 `color-scheme` 与变量（`--doc-bg/--doc-text/--doc-link/--doc-hr`），正文、链接、`<hr>`、表格边框、音标字体统一用变量。
- 暗色时追加「深色覆盖层」放在词典自带 CSS **之后**，对 `body/div/table/td/常见 class` 的硬编码浅色做精准 `!important` 覆盖（借鉴 GoldenDict/欧路的 CSS 前缀隔离思路，但 ours 更可控）。
- **卡片外壳同步**：把 `.result-body/.entry-frame/高亮渐变/骨架屏/展开全文按钮` 改为受 `html[data-content-theme]` 驱动的变量，避免「外壳黑、卡片白」割裂。
- 切换主题时复用「保存设置后重跑当前查询」逻辑（`app.js:1769`）重建 iframe。
- **CSP 无需改**：`style-src 'unsafe-inline'` 与 `frame-src data/about` 已满足内联样式与 srcdoc。
- 配置落位：推荐写进核心 `Settings`（新增字段带 `#[serde(default)]`，`config.json` 向后兼容）；若想零后端改动，可暂放 localStorage。建议前者，与「主题/字号」保持一致。
- **不采用**「一行 `filter:invert(1) hue-rotate(180deg)`」作为主方案（调研确认会反相图片/图标/emoji、导致去饱和），仅可作为「强制深色」的兜底开关。

---

## 8. 性能优化（在已有基础上加码）

| # | 优化 | 阶段 | 说明 |
|---|---|---|---|
| 1 | iframe 生命周期回收 | P0 | 改 `rootMargin:'420px'` 为两级：临近预建 DOM、**真正进入视口才 `srcdoc` 加载**，离屏较远则销毁 iframe 保留 `srcdoc` 占位，限制同时活跃数量（如 6 个） |
| 2 | 渲染缓存 | P0 | 对 `buildDocument(dictId, headword)` 结果做内存 LRU（如 50 条），重复查询免于重复 DOMParser 清洗与 CSS 重写 |
| 3 | CSS 隔离 | P0 | 结果卡片加 `contain: content` / `content-visibility: auto`，限制布局/绘制范围 |
| 4 | 高度计算批处理 | P0 | `sizeFrame()` 的 ResizeObserver 回调用 `requestAnimationFrame` 合并写入 |
| 5 | 联想 DOM 上限 | P0 | `renderSuggestions` 限制节点数并复用，避免反复重建高亮 DOM |
| 6 | 取消轮询 | P1 | 用 Tauri FS watcher/`dict-changed` 事件替代 60s 轮询，或查询活跃/模态框打开时跳过刷新（`init()` `app.js:2151`） |
| 7 | 索引事件 | 保持 | 后端已 80ms 节流，前端仅在必要时 `updateDictRow` |

---

## 9. 易用性与现代化细节

- **收藏 / 生词本（P1，前端 localStorage 起步）**：卡片头加星标收藏；侧栏加「收藏 / 最近」切换视图；生词本支持移除与随机复习（借鉴欧路/DictTango）。
- **阅读模式（P2）**：单词典沉浸阅读＋中文词头拼音排序＋图片词典模式（DictTango 启发）。
- **每词典字体覆盖（P2）**：在「释义外观」里按词典设置字体，解决排版不理想词典的问题（MDict/DictTango 强项）。
- **无障碍（P0）**：模态框加焦点陷阱与 `aria-labelledby`；折叠卡加 `aria-expanded`；全局 `focus-visible` 描边；toast/索引完成用 `aria-live`；正文对比度达 WCAG AA 4.5:1；保留 `prefers-reduced-motion`。
- **微交互**：卡片头彩色 accent 区分词典、平滑折叠、骨架屏、空状态引导、所有异步操作反馈。
- **代码结构（可选 P2）**：`app.js` 已 2169 行，建议按 `theme / dicts / results / menus / settings` 拆为 ES 模块（无打包器，Tauri 资源协议下可用 `<script type="module">`），降低后续迭代成本。

---

## 10. 实施优先级与后端影响

| 阶段 | 内容 | 是否动 Rust |
|---|---|---|
| **P0（纯前端，零后端）** | 暗色内容主题（默认保持现状）、锚点/chips 导航、历史前进后退、键盘重排与卡片导航、筛选时重排、iframe 回收/缓存/contain、无障碍 | 否（配置先放 localStorage 亦可） |
| **P1（小改动）** | 词条上一条/下一条、收藏/生词本、词典分组、内容主题配置进核心 Settings、FS watcher 替代轮询 | 是：`commands.rs`+`lib.rs` 增命令；`Settings`/`ConfigFile` 加字段（均 `#[serde(default)]`） |
| **P2（增强）** | 词典管理面板、全文搜索（建议 sidecar）、阅读模式、每词典字体、代码模块化 | 是 |

**关键风险与规避**

1. 默认保持 `alwaysLight`/现状，杜绝暗色改动造成回归。
2. 词典自带 CSS 可能有硬编码浅色，深色覆盖层需按常见 class 精准覆盖，并用 `resources/mdx` 样本词典回归。
3. CSP 不需改（内联样式与 srcdoc 已在白名单）。
4. 分组/相邻词条/内容主题等配置字段用 `#[serde(default)]`，兼容旧 `config.json`。

---

## 11. 附：核心代码位置速查

- 外壳/布局：`ui/index.html`、`ui/styles.css`（主题变量 `:5-68`、结果卡 `:1070-1144`、媒体查询 `:1778-1808`）
- 主题/历史：`app.js` `applyTheme():209`、`loadHistory()/rememberSearch():216-246`
- 词典列表与顺序：`dictRow():303`、`renderDicts():344`、`applyDrag():2012`、`persistOrder():522`
- 查询与联想：`runSearch():716`、`renderResults():756`、`renderSuggestions():614`
- 结果卡与 iframe：`resultCard():865`、`loadCard():936`、`sizeFrame():986`、`buildDocument():1149`、`baseCss():186`
- IPC：`src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`（`generate_handler!`）
- 核心能力：`crates/mdict-core/src/registry.rs`（`Settings`、`DictionaryInfo`、`query/suggest/lucky`）、`index/format.rs`（`key/definition/random`）
- 事件与进度：`src-tauri/src/indexer.rs`、`app.js setupTauriListeners():2091`
