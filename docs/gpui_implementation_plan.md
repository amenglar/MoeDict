# GPUI 原生版实施方案（MoeDict Native）

> 分支：`feat/gpui-native` · 基线：`45a38d5`（Tauri 版） · 日期：2026-11-24

## 1. 结论与范围

在原设计 `docs/gpui_design.md` 的基础上做了调研与收敛后的**可执行方案**：

- **Phase 0 已通过（go）**：`gpui = 0.2.2` + `gpui-component = 0.5.1` 在本机 Windows/MSVC + Rust 1.92.0
  完整 `cargo build` 成功；Linux 侧 Rust 1.100.0 可用（需 wayland/x11/Vulkan 开发库）。
  `StyledText::with_default_highlights` / `with_runs`、`Input`、原生文件对话框、
  图片字节加载、剪贴板等能力均已验证存在。
- **目标**：在独立分支上交付一个可运行、可验收的原生版本；**不合并回 `main`**。
- **功能对齐**：搜索/联想/查询、词典增删改查排序、索引进度、MDD 资源、
  图片/发音/交叉引用、历史前进后退、设置与主题、快捷键等与现有 WebView 版保持一致。

## 2. 对原方案的优化与偏离（重要）

| # | 原设计 | 本方案 | 理由 |
|---|---|---|---|
| 1 | Phase 1 自研 mdx/mdd 解析 | **直接复用 `mdict-core`** | 仓库已有 mmaps + zstd + 只读索引 + 重定向 + 资源读取，重复实现无收益。 |
| 2 | lightningcss + 完整级联 + 多词典专用适配器 | **轻量 CSS 提取 + 通用 DOM→AST + 类名启发式** | 100% CSS 还原是非目标；手写极简选择器匹配（`.class`/`tag`/`#id`）即可覆盖绝大多数内联色/粗斜体，避免引入大型 CSS 引擎；保留 Fallback 保证不丢内容。 |
| 3 | AST 磁盘缓存 postcard + LRU | **先做内存 LRU（200 条）+ 索引本身的 zstd 缓存** | `.idx` 已缓存压缩 HTML；二次解析成本很低。磁盘 AST 缓存列为 Phase 6，不在首版引入。 |
| 4 | 5 套主题一次到位 | **3 套基础主题（浅色/深色/护眼） + 系统跟随，预留扩展** | 主题只是色板数据，后续扩展成本极低；先保证对比度与排版层级正确。 |
| 5 | 自实现输入控件 | **使用 `gpui-component::InputState`** | 中文 IME 是刚需，自研输入控件无法可靠支持；组件库版本已锁定。 |
| 6 | Tauri IPC / 自定义协议 / iframe | **全部移除**：Rust 进程内直接调用 core、`read_resource` 返回字节后交给 GPUI 图片缓存 | 无 WebView、无 IPC、无 HTTP、无 CSP；内存与启动路径大幅缩短。 |
| 7 | 音标字体需额外下载 | **复用仓库已有 `ui/fonts/KingsoftPhoneticPlain.ttf`（OFL/用户自备）** | 打包进原生二进制，无外部依赖；找不到时回退系统字体。 |
| 8 | 每词条虚拟滚动 | **结果卡片按需渲染 + 折叠 + 长词条分块** | 词典查询通常 1–8 条命中；`list()` 虚拟滚动作为 Phase 6 优化，首版先用折叠控制长词条开销。 |

> 原则：**内容永远不能丢**。任何无法语义化的 DOM 子树降级为 `Fallback(RichText)`，纯文本保底。

## 3. 架构

```
crates/mdict-core/       （原样复用）
   └─ DictionaryStore: 配置 / 索引 / 查询 / 联想 / 资源
crates/mdict-native/     （新增）
   ├─ main.rs          启动、字体注册、窗口
   ├─ app.rs           根视图、全局状态、事件总线
   ├─ paths.rs         配置/索引/日志目录（与 Tauri 版同目录，兼容旧数据）
   ├─ theme.rs         语义色板 + 字号/间距
   ├─ ast.rs           DictEntry / DictBlock / RichText（serde 可序列化）
   ├─ html.rs          html5ever DOM → AST；轻量 CSS；类名启发式；降级
   ├─ entry_view.rs    AST → GPUI 元素（StyledText 单次排版）
   ├─ resources.rs     图片字节缓存 / 音频播放（rodio）
   ├─ indexer.rs       后台单线程索引 + 进度轮询
   ├─ history.rs       历史与前进后退
   ├─ views/           sidebar / search / results / dialogs / toasts
   └─ assets/          字体
```

数据流：`Input → suggest/query(core) → html→AST(LRU) → GPUI 渲染`，全程零 WebView。

## 4. 功能对齐矩阵

| 现有功能 | 原生版实现 |
|---|---|
| 添加 MDX / 扫描文件夹 / 拖入文件 | `App::prompt_for_paths` + `on_drop::<ExternalPaths>` → `store.add_paths` |
| 启用/停用、重命名、删除、排序、筛选 | 侧栏原生控件 + 菜单 + 对话框 |
| 索引进度、取消、重建、自动索引 | 后台线程 + UI 轮询进度；菜单操作 |
| 查询（多词典按序）、联想、手气 | 直接调用 `store.query/suggest/lucky`；140ms 防抖 + 竞态序号 |
| 释义富文本、图片、发音、交叉引用、锚点 | AST 渲染 + `img(Arc<Image>)` + rodio + `data-entry` 等价动作 + 滚动定位 |
| 折叠/展开、结果导航 | 卡片折叠按钮 + 侧栏/顶部跳转 |
| 历史、前进后退、最近搜索 | 内存 + 本地 JSON（替代 localStorage） |
| 设置（主题/字号/联想数/默认展开/自动索引/显示未收录） | 原生设置对话框，复用 core `Settings` 持久化 |
| 暗色模式、内容主题 | 语义色板即时切换；跟随系统 |
| 快捷键 | GPUI `bind_keys` / `actions!`（Ctrl/Cmd+L/K、/、Alt+←/→、Ctrl+,、Ctrl+O 等） |
| 复制释义 | `cx.write_to_clipboard` |
| 打开所在文件夹 / 数据目录 / 外部链接 | `cx.reveal_path` / `cx.open_url` |

## 5. 性能设计

1. 索引 mmap 与 zstd 释义由 core 负责；查词只解压当前词条。
2. AST 内存 LRU（200 条），按 `(dict_id, headword)` 缓存；同一释义不重复解析。
3. CSS 规则每本词典解析一次并缓存（`OnceCell`/`Mutex<HashMap>`）。
4. 内联样式合并为**一个 `StyledText`**，避免每 `<span>` 一个元素。
5. 图片按 `(dict_id, path)` 缓存解码结果；限制缓存条目与像素上限。
6. 音频只在点击时读取 `.mdd`/文件系统，不预加载。
7. 后台索引线程与 UI 解耦；进度节流 ~80ms。
8. 长释义默认折叠超长块，避免一次性排版上百段。

内存预算（目标）：进程 + GPUI ~30MB，字体 ~10MB，索引 mmap ~5–20MB，
AST/图片缓存 ~15MB，合计 **< 80MB**（实测数据见验收报告）。

## 6. 阶段划分与当前状态

- **P0 调研（已完成）**：工具链、API、可行性验证、方案收敛。
- **P1 骨架（已完成）**：`crates/mdict-native`、workspace 接线、路径兼容、根视图、三套主题、`--mdx-dir`/`MOEDICT_MDX_DIR`。
- **P2 数据→AST（已完成）**：html5ever + 轻量 CSS（class/id/属性/后代选择器、`display:none`、`rgb()`/hex 语义色映射）+ 通用适配器 + Fallback + 外部 CSS 加载 + 4 个单元测试。
- **P3 渲染（已完成）**：`StyledText` 单次排版、块级渲染、图片字节缓存（LRU 120）、rodio 音频、语义色板。
- **P4 交互（已完成）**：侧栏、搜索/联想、结果卡片、历史前进后退、快捷键、菜单/对话框/Toast。
- **P5 集成（已完成）**：后台单线程索引 + 进度轮询、原生文件对话框、外部文件拖放、设置持久化、CLI；
  标签筛选（`query_with_tags`/`suggest_with_tags`/`lucky_with_tags`）与 Speex → WAV 按需转码。
- **P6 验收（进行中）**：`cargo test` 全部通过（core 10 + native 4）；`cargo fmt --all` 完成；
  Windows 实机启动验证通过（GPU 初始化正常、无崩溃）；真实词典覆盖率脚本已跑通。
- **后续（不在首版）**：磁盘 AST 缓存、虚拟滚动、更多专用适配器、生词本、精确锚点滚动。

### 真实词典调研结果（牛津高阶英汉双解 8th，样本目录由用户提供）

- MDX：92,667 条，UTF-8；配套 `O8C.css` 使用 3 级后代选择器、属性选择器 `[display="block"]`、
  `display:none`、`rgb()`/`#C76E06` 等色值 —— 均已纳入转换器支持。
- 抽样前 100 条：空条目 0；识别出 headword 53、sense 52、image 19、paragraph 102。
- 该词典通过 `<link rel="stylesheet" href="O8C.css">` 引用外部 CSS，原生版会通过
  `DictionaryStore::read_resource` 读取同目录/MDD 中的 CSS 并参与规则匹配。
- 调研/验收工具：`cargo run --release -p moedict-native --example coverage -- <file.mdx> --limit N`。

## 7. 验证方式

```bash
cargo run -p moedict-native -- --mdx-dir ./resources/mdx
cargo test -p mdict-native
cargo fmt --all -- --check
```

> Windows 若通过 `\\wsl.localhost\...` UNC 路径运行 cargo，增量编译锁可能报错；
> 设置 `CARGO_INCREMENTAL=0` 和 `CARGO_TARGET_DIR` 为本地 NTFS 路径即可。

### 构建产物

- Windows release：`dist/moedict-native.exe`（约 18 MB）、`dist/coverage.exe`（调研工具，约 1.9 MB）。
  `dist/` 已加入 `.gitignore`，不会进入分支提交。
- Linux：源码可 `cargo check`，完整链接仍需系统库 `libxkbcommon-dev`、`libwayland-dev`、
  `libx11-xcb-dev` 等（当前环境无 sudo，未安装，故未产出 Linux 可执行文件）。
- 构建兼容性补丁：`vendor/xattr`（`ENOATTR` 在 libc ≥ 0.2.190 被移除），通过根 `Cargo.toml`
  的 `[patch.crates-io]` 生效；`rodio` 仅在 Windows/macOS 编译，Linux 下发音功能降级为提示。

验收清单：
- [ ] 冷启动无 WebView 进程；常驻内存显著低于 Tauri 版
- [ ] 查询/联想结果与 Tauri 版一致（同一词典、同一词）
- [ ] 图片、发音、跳转、锚点、折叠、复制可用
- [ ] 词典 CRUD、索引进度、设置持久化可用
- [ ] 浅色/深色/护眼主题与系统跟随可用
- [ ] `main` 分支未改动，未合并

## 8. 风险

- GPUI 0.2.x 仍 pre-1.0；已锁定精确版本并提交 `Cargo.lock`。
- Linux 需额外系统库；Windows 已验证，Linux 为次优先目标。
- `spx` 发音解码依赖系统/解码器，不支持时降级提示（与原版一致）。
- 畸形 HTML 可能降级为纯文本；遵循“内容不丢”原则。
