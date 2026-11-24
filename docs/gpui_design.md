# mdict 词典 gpui 原生渲染方案

> 版本：v1.0  
> 日期：2026-11-24  
> 目标：以 gpui 替代 Tauri WebView，实现低内存、高颜值、原生体验的 mdict 词典阅读器

---

## 1. 背景与问题

### 1.1 现状
- 项目处理 mdict 词典（`.mdx` / `.mdd`），词条内容为出版商提供的 **HTML + CSS**。
- 当前使用 Tauri，每个窗口都会启动系统 WebView（WebView2 / WKWebView / WebKitGTK）。
- 痛点：**内存占用大**（空窗口即可达 100–300MB），启动慢，与原生 UI 割裂。

### 1.2 为什么不能直接"用 gpui 渲染 HTML"
- gpui **没有 HTML 解析器，没有 CSS 引擎**，它是自绘的原生 UI 框架（Flexbox + 自有文本排版引擎）。
- 社区现有方案（`oxidize-html`、`gpui-html` 等）只支持**受限 HTML 子集**，无法覆盖 mdict 的复杂 CSS（嵌套 table、自定义字体、音标、`!important` 滥用等）。
- **结论**：不要做"浏览器"，要做"语义提取 + 原生重排版"。

---

## 2. 目标与非目标

### 2.1 目标（Must Have）
| 项 | 指标 |
|---|---|
| 内存占用 | 常驻 < 80MB（对比 Tauri 降低 60%+） |
| 冷启动 | < 500ms 打开主界面 |
| 查词延迟 | < 50ms（命中缓存 < 5ms） |
| 滚动帧率 | 长词条（100+ 释义）稳定 60fps |
| 词典覆盖 | 主流词典 95% 内容正确显示 |
| 排版质量 | **优于**原版（统一字体、间距、暗色主题） |

### 2.2 非目标（Won't Do）
- ❌ 100% 像素级还原原词典 CSS
- ❌ 支持 JavaScript / 交互式 HTML
- ❌ 支持 CSS Grid、动画、渐变、复杂伪类
- ❌ 兼容所有冷门/劣质词典的畸形 HTML（降级为纯文本即可）

---

## 3. 总体架构

```
┌─────────────┐
│ .mdx / .mdd │  词典文件（二进制索引 + 压缩 HTML 块）
└──────┬──────┘
       │ 按 key 查 offset，只解压当前词条
       ▼
┌─────────────────────────────────────────┐
│ Layer 1  解析与清洗                      │
│  html5ever / kuchiki  → DOM             │
│  lightningcss         → CSS Rules       │
│  简化级联计算          → Computed Style  │
└──────┬──────────────────────────────────┘
       ▼
┌─────────────────────────────────────────┐
│ Layer 2  语义化中间表示（核心）           │
│  DOM + Style → DictEntry AST            │
│  规则引擎：按词典家族（牛津/朗文/…）适配   │
└──────┬──────────────────────────────────┘
       │ AST 可序列化缓存（bincode / postcard）
       ▼
┌─────────────────────────────────────────┐
│ Layer 3  gpui 原生渲染                   │
│  块级 → div().flex_col()                │
│  内联 → 富文本 TextRun / StyledText      │
│  主题 → DictTheme                       │
└─────────────────────────────────────────┘
```

### 3.1 分层理由
- **Layer 1** 容错：mdict 的 HTML 常不规范，需 HTML5 容错解析。
- **Layer 2** 是灵魂：把 `<span class="e">` 这种无语义标签转成 `Example`，把"渲染问题"变成"数据建模问题"。
- **Layer 3** 极简：gpui 只消费 AST，运行时**零 HTML/CSS 解析开销**。

---

## 4. 技术选型

| 层 | 组件 | 选型 | 备注 |
|---|---|---|---|
| 文件 | mdx/mdd 解析 | crates.io 上的 mdict/mdx 解析库，或自实现 | 格式并不复杂，自实现约 500 行；需自行验证候选库维护状态 |
| 文件 | 解压 | `flate2`(zlib) / `lzo` / `brotli` | mdx 常见 zlib 与 lzo 压缩 |
| L1 | HTML | `html5ever` + `kuchiki`（或 `scraper`） | kuchiki 提供 CSS 选择器查询，API 友好 |
| L1 | CSS | `lightningcss` | Mozilla/Parc 出品，解析快、结构化输出 |
| L1 | 级联 | 自实现简化版 | mdict 选择器多为 `.class` / `tag` / 后代，无需完整 specificity |
| L2 | AST 序列化 | `serde` + `postcard` / `bincode` | 缓存到磁盘 |
| L3 | UI | `gpui`（Zed 团队） | 自绘 GPU 渲染 |
| L3 | 组件库 | `gpui-component` / `gpui-kit`（Longbridge） | 提供滚动、列表、主题等基础设施，可选 |
| 通用 | 字体 | 系统字体 + 内置音标字体 | Charis SIL / Doulos SIL / Noto 系列 |

---

## 5. Layer 2：语义化 AST 设计

### 5.1 数据结构

```rust
/// 一个词条
pub struct DictEntry {
    pub headword: String,
    pub blocks: Vec<DictBlock>,
    pub source_dict: DictId,
}

/// 块级元素
pub enum DictBlock {
    /// 词头行：单词 + 音标 + 词性
    HeadwordLine {
        text: String,
        phonetics: Vec<Phonetic>,
        pos: Option<String>,
        audio: Option<AudioRef>,
    },
    /// 义项
    Sense {
        number: Option<String>,      // "1." "(a)"
        pos: Option<String>,
        definition: RichText,
        examples: Vec<Example>,
        children: Vec<DictBlock>,    // 子义项
    },
    /// 例句（独立成块时）
    Example(Example),
    /// 参见 / 交叉引用
    CrossRef { target: String, label: String },
    /// 语法/用法说明框
    Note { kind: NoteKind, content: RichText },
    /// 图片
    Image { src: ImageRef, caption: Option<String> },
    /// 表格（语法表、变形表）
    Table { header: Option<Vec<RichText>>, rows: Vec<Vec<RichText>> },
    /// 分隔线
    Divider,
    /// 兜底：无法语义化的内容
    Fallback(RichText),
}

pub struct Example {
    pub source: RichText,        // 原文
    pub target: Option<RichText>,// 译文（双语词典）
}

pub struct Phonetic {
    pub text: String,
    pub variant: PhoneticVariant, // UK / US / IPA / 拼音
    pub audio: Option<AudioRef>,
}

/// 内联富文本 = 带样式的文本片段序列
pub struct RichText {
    pub runs: Vec<StyledRun>,
}

pub struct StyledRun {
    pub text: String,
    pub style: InlineStyle,
    pub link: Option<LinkTarget>, // 点击跳转到其他词条
}

#[derive(Clone, Default)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub super_sub: Option<SuperSub>, // 上标/下标
    pub color: Option<SemanticColor>,// 语义色，而非具体 RGB
    pub font_role: FontRole,         // Serif / Sans / Phonetic / Mono
    pub size_delta: i8,              // 相对基准字号的增减
}

/// 关键：颜色用语义而非硬编码，才能支持主题切换与暗色模式
pub enum SemanticColor {
    Primary, Secondary, Muted, Accent,
    Headword, Phonetic, Pos, Example, Link, Warning,
}
```

### 5.2 设计要点

1. **颜色语义化**：原 CSS 的 `color: #c00` 不直接保留 RGB，而是映射为 `SemanticColor::Headword`。这样才能一键切换暗色主题，否则暗色下会出现"黑底黑字"。
2. **字体角色化**：`FontRole::Phonetic` 而非 `font-family: "Doulos SIL"`，便于统一替换与回退。
3. **保留 Fallback**：任何无法识别的结构降级为 `Fallback(RichText)`，保证**内容不丢失**。
4. **可序列化**：整个 AST `#[derive(Serialize, Deserialize)]`，用于磁盘缓存。

### 5.3 适配器机制（应对不同词典）

```rust
pub trait DictAdapter {
    fn matches(&self, dict_meta: &DictMeta, sample_html: &str) -> bool;
    fn convert(&self, dom: &Node, css: &CssRules) -> DictEntry;
}

// 注册表
pub fn adapters() -> Vec<Box<dyn DictAdapter>> {
    vec![
        Box::new(OxfordAdapter),      // OALD / ODE
        Box::new(LongmanAdapter),     // LDOCE
        Box::new(CollinsAdapter),
        Box::new(CambridgeAdapter),
        Box::new(MerriamWebsterAdapter),
        Box::new(ChineseDictAdapter), // 现汉 / 成语
        Box::new(GenericAdapter),     // 通用兜底
    ]
}
```

- 通过词典元数据（标题、`class` 命名特征）自动选择适配器。
- `GenericAdapter`：基于标签语义（`b/i/ul/table`）+ 计算样式的通用转换，覆盖未适配词典。
- 允许用户手动指定适配器（UI 下拉选择）。

---

## 6. Layer 3：gpui 渲染设计

### 6.1 块级 → Flexbox

```rust
fn render_block(block: &DictBlock, theme: &DictTheme, cx: &mut Context) -> AnyElement {
    match block {
        DictBlock::HeadwordLine { text, phonetics, pos, .. } => {
            div()
                .flex().flex_row().items_baseline().gap_2()
                .child(
                    div().text_size(theme.size.headword)
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.color.headword)
                        .child(text.clone())
                )
                .children(phonetics.iter().map(|p| render_phonetic(p, theme, cx)))
                .when_some(pos.clone(), |el, pos| el.child(render_pos(&pos, theme)))
                .into_any_element()
        }

        DictBlock::Sense { number, definition, examples, children, .. } => {
            div().flex().flex_row().gap_2()
                .when_some(number.clone(), |el, n| {
                    el.child(div().text_color(theme.color.muted).child(n))
                })
                .child(
                    div().flex().flex_col().gap_1().flex_1()
                        .child(render_rich_text(definition, theme, cx))
                        .children(examples.iter().map(|e| render_example(e, theme, cx)))
                        .children(children.iter().map(|b| render_block(b, theme, cx)))
                )
                .into_any_element()
        }
        // ... 其余分支
    }
}
```

### 6.2 内联 → 单次富文本绘制（性能关键）

**不要**把每个 `<span>` 拆成一个 `div`。一整段释义应该只产生**一个**文本布局：

```rust
fn render_rich_text(rt: &RichText, theme: &DictTheme, cx: &mut Context) -> AnyElement {
    // 使用 gpui 的 StyledText / TextRun 能力，
    // 将多个样式的片段合并为一次文本排版与绘制
    let mut text = String::new();
    let mut runs = Vec::new();
    for r in &rt.runs {
        let start = text.len();
        text.push_str(&r.text);
        runs.push((start..text.len(), r.style.to_gpui_text_style(theme)));
    }
    // 具体 API 以所用 gpui 版本为准（StyledText::new + highlight ranges）
    StyledText::new(text).with_runs(runs).into_any_element()
}
```

> ⚠️ gpui 的富文本 API 在不同版本差异较大（`StyledText`、`TextRun`、`HighlightStyle` 等）。
> 实施前请以 Zed 源码中 Markdown 预览 / 编辑器高亮的实现为准。

### 6.3 字体回退链

```rust
pub fn font_stack(role: FontRole) -> Vec<&'static str> {
    match role {
        FontRole::Phonetic => vec!["Charis SIL", "Doulos SIL", "Gentium Plus", "Noto Sans", "sans-serif"],
        FontRole::Serif    => vec!["Source Serif", "Noto Serif", "Songti SC", "serif"],
        FontRole::Sans     => vec!["Inter", "Noto Sans", "PingFang SC", "Microsoft YaHei", "sans-serif"],
        FontRole::Mono     => vec!["JetBrains Mono", "Consolas", "monospace"],
    }
}
```

- **音标必须内置字体**（打包 Charis SIL，OFL 许可），否则 IPA 符号会显示为豆腐块。
- CJK 与拉丁混排时注意基线对齐（`.items_baseline()`）。

### 6.4 交互

| 元素 | 交互 |
|---|---|
| `CrossRef` / `LinkTarget` | 点击 → 查词跳转，维护前进/后退历史栈 |
| `AudioRef` | 点击 → 播放发音（`.mdd` 内音频或在线 TTS） |
| 词头 | 右键 → 复制 / 加入生词本 |
| 图片 | 点击 → lightbox 放大 |
| 全文 | 支持鼠标划选复制（gpui 文本选择能力） |

---

## 7. 主题系统（美观性的核心）

### 7.1 设计理念
**放弃还原原词典 CSS，改为提供统一的、精心调校的排版主题。**  
这反而比 WebView 里原样渲染更好看，因为可以针对 gpui 的渲染特性做精细优化。

```rust
pub struct DictTheme {
    pub name: &'static str,
    pub color: ColorScheme,
    pub size: SizeScale,
    pub spacing: SpacingScale,
    pub fonts: FontConfig,
}

pub struct ColorScheme {
    pub bg: Rgba,
    pub headword: Rgba,
    pub phonetic: Rgba,
    pub pos: Rgba,
    pub definition: Rgba,
    pub example: Rgba,
    pub example_translation: Rgba,
    pub link: Rgba,
    pub muted: Rgba,
    pub accent: Rgba,
    pub note_bg: Rgba,
    pub divider: Rgba,
}
```

### 7.2 内置主题
| 主题 | 风格 |
|---|---|
| **Oxford Classic** | 衬线字体、深蓝词头、传统词典感 |
| **Modern Light** | 无衬线、大间距、卡片化义项 |
| **Eudic Dark** | 暗色、柔和对比、护眼 |
| **Compact** | 小字号、紧凑行距，一屏更多内容 |
| **E-Reader** | 米黄底、衬线、适合长时间阅读 |

### 7.3 排版细节清单（决定"美观"的关键）
- [ ] 义项编号与正文**悬挂缩进**对齐
- [ ] 例句用左侧竖线或淡色背景区分，而非单纯斜体
- [ ] 音标使用专用字体，字号略小于词头
- [ ] 词性标签用小圆角 badge（背景色 + 小字号）
- [ ] 双语例句原文/译文行距区分（译文更紧凑、颜色更淡）
- [ ] Note/用法框用左边框强调条 + 淡背景
- [ ] 义项之间间距 > 例句之间间距（视觉层级）
- [ ] 暗色模式下所有颜色重新调校（不是简单反色）

---

## 8. 性能与内存优化清单

| # | 优化项 | 做法 | 收益 |
|---|---|---|---|
| 1 | **按需读取词条** | 利用 mdx 的 key→offset 索引，只解压当前词条 | 内存从 GB 级 → MB 级 |
| 2 | **AST 磁盘缓存** | 首次解析后序列化 AST 到 `cache/{dict_id}/{key_hash}.bin` | 二次查词跳过 HTML/CSS 解析，快 10–50× |
| 3 | **LRU 内存缓存** | 最近 N 个词条 AST 常驻（N=50–200，可配） | 前进/后退瞬时响应 |
| 4 | **CSS 只解析一次** | 每本词典的 CSS 在打开时解析一次并常驻 | 避免每词条重复解析 |
| 5 | **样式对象复用** | `Arc<TextStyle>` + 样式池，相同样式共享 | 减少 80% 样式分配 |
| 6 | **虚拟滚动** | 长词条只渲染视口内块（用 gpui `uniform_list` / `list`） | 100+ 义项仍 60fps |
| 7 | **图片懒加载** | 进入视口才解码；磁盘缓存 + 尺寸限制 | 避免峰值内存 |
| 8 | **音频按需** | 不预加载 `.mdd` 音频，点击才读取 | 启动内存降低 |
| 9 | **索引 mmap** | mdx 索引用内存映射，不做全量拷贝 | 大词典（百万词）友好 |
| 10 | **后台解析** | 解析放线程池，不阻塞 UI 线程 | 输入不卡顿 |
| 11 | **搜索防抖** | 输入 150ms 防抖 + 前缀索引 | 减少无效查询 |
| 12 | **避免深拷贝** | AST 用 `Arc` 共享，渲染层只读引用 | 减少克隆开销 |

### 8.1 内存预算参考
```
基础进程 + gpui 渲染上下文     ~25 MB
字体（含内置音标字体）          ~8 MB
当前词典索引（mmap，按需页入）   ~5–20 MB
AST LRU 缓存（100 词条）        ~3 MB
图片缓存（限制上限）            ~10 MB
──────────────────────────────────
合计目标                       < 80 MB
```

---

## 9. 降级与容错策略

```
解析成功且适配器匹配   → 完整语义渲染（最佳效果）
        ↓ 失败
GenericAdapter 通用转换 → 保留基础样式（可接受）
        ↓ 失败
HTML → 纯文本 + 基础换行 → 内容完整，无样式
        ↓ 失败
显示原始 HTML 源码 + "该词条解析失败" 提示 + 反馈按钮
```

**原则：内容永远不能丢。** 宁可样式简陋，不可显示空白。

### 9.1 可观测性
- 记录每个词典的解析成功率、Fallback 比例。
- 提供"导出问题词条"功能，便于迭代适配器。
- Debug 模式可并排显示：原始 HTML / AST / 渲染结果。

---

## 10. 实施路线图

### Phase 0：调研（1 周）
- [ ] 收集 5–8 本主流 mdict 词典样本
- [ ] 统计标签分布、class 命名规律、CSS 复杂度
- [ ] 验证 gpui 富文本 API 能力边界（写最小 demo）
- [ ] **产出**：AST 覆盖范围清单 + gpui 可行性确认

> ⚠️ **Phase 0 是 go/no-go 决策点**。若 gpui 富文本能力不足以支撑内联混排，需重新评估。

### Phase 1：数据层（1–2 周）
- [ ] mdx/mdd 解析（索引、解压、资源提取）
- [ ] 词条查询 API + 前缀搜索
- [ ] 单元测试：能正确取出任意词条 HTML

### Phase 2：转换层（2–3 周）
- [ ] HTML/CSS 解析 + 简化级联
- [ ] `DictEntry` AST 定义与序列化
- [ ] `GenericAdapter` 通用转换
- [ ] `OxfordAdapter` + `LongmanAdapter`（先啃两本最主流的）
- [ ] **产出**：离线工具，批量把词条转 AST 并输出可读文本，人工抽检

### Phase 3：渲染层（2–3 周）
- [ ] gpui 基础窗口 + 搜索框 + 结果列表
- [ ] 块级渲染 + 内联富文本渲染
- [ ] 字体回退 + 音标显示
- [ ] 一套主题跑通完整词条

### Phase 4：体验打磨（2 周）
- [ ] 5 套主题 + 暗色模式
- [ ] 交叉引用跳转 + 历史前进后退
- [ ] 图片、音频、表格
- [ ] 文本选择复制、生词本

### Phase 5：性能优化（1–2 周）
- [ ] AST 缓存 + LRU
- [ ] 虚拟滚动
- [ ] 内存 profiling，对照预算表逐项优化
- [ ] **产出**：性能报告（内存 / 启动 / 查词延迟 / 帧率）

### Phase 6：扩展适配（持续）
- [ ] 柯林斯、剑桥、韦氏、现汉…
- [ ] 用户反馈驱动的问题词条修复

**总计：约 10–14 周**（单人全职估算）

---

## 11. 风险与对策

| 风险 | 等级 | 对策 |
|---|---|---|
| gpui 富文本 API 不稳定/文档缺失 | 🔴 高 | Phase 0 先验证；锁定 gpui 版本；参考 Zed 源码而非文档 |
| gpui 文本选择/复制能力不足 | 🟡 中 | 早期验证；必要时自实现选择层 |
| 词典 HTML 过于畸形，适配器工作量大 | 🟡 中 | 坚持 95% 目标；Fallback 保底；社区共建适配器 |
| 音标/CJK 字体显示异常 | 🟡 中 | 内置 OFL 字体；完善回退链；多平台测试 |
| gpui 跨平台成熟度（尤其 Linux/Windows） | 🟡 中 | 三平台 CI；早期在目标平台验证 |
| 表格渲染复杂（变形表、语法表） | 🟢 低 | 简化为 Flex 网格，不追求完整 table 语义 |
| 开发周期超预期 | 🟡 中 | 分阶段可交付；Phase 3 结束即可用 |

---

## 12. 备选方案对比

| 方案 | 内存 | 排版还原度 | 开发成本 | 原生感 | 结论 |
|---|---|---|---|---|---|
| **Tauri + WebView**（现状） | 🔴 高 | 🟢 100% | 🟢 低 | 🔴 差 | 内存痛点无解 |
| **gpui + 语义 AST**（本方案） | 🟢 极低 | 🟡 95%* | 🔴 高 | 🟢 优 | **推荐** |
| gpui + oxidize-html 直渲 | 🟢 低 | 🔴 30% | 🟡 中 | 🟢 优 | CSS 覆盖不足，不可行 |
| gpui + WebView 混合（复杂词条降级） | 🟡 中 | 🟢 100% | 🟡 中 | 🟡 中 | 折中方案，见下 |
| egui / Iced | 🟢 低 | 🔴 差 | 🔴 高 | 🟡 中 | 富文本能力弱于 gpui |
| 原生控件（NSAttributedString 等） | 🟢 低 | 🟡 中 | 🔴 高 | 🟢 优 | 无法跨平台 |

\* 语义 AST 方案的"还原度"指**内容完整性**，视觉上是重新设计的统一排版，实际观感可能**优于**原版。

### 12.1 折中方案：渐进式迁移
如果担心一步到位风险太大，可采用：
1. **主界面用 gpui**（搜索、列表、设置、生词本）——立即降低大部分内存
2. **词条渲染先用轻量 WebView**（单实例复用，而非每窗口一个）
3. 逐步把语义 AST 渲染做成熟，**默认切换到原生**，WebView 作为"原始样式"可选开关

这样既能快速见效，又保留 100% 还原的兜底能力。

---

## 13. 参考资源

### 13.1 直接相关
- **dicto** — 用 gpui 写的离线 mdict 词典应用，内含 `gpui/src/html/render.rs`，**最接近本需求的参考实现**  
  搜索结果中出现过 `github.com/logi-camp/dicto` 与 `github.com/mohamadkhani/dicto` 两个路径，**请以实际可访问仓库为准**
- **gpui-component / gpui-kit**（Longbridge 开源）— 声明支持 "Native Markdown and HTML rendering"，可复用其富文本与滚动基础设施  
  `github.com/longbridge/gpui-kit`
- **oxidize-html** — gpui 的 HTML 渲染引擎尝试，可参考其布局/绘制思路（但 CSS 覆盖不足）  
  `github.com/ghostvox/oxidize-html`

### 13.2 学习材料
- **Zed 源码** — gpui 富文本排版的权威参考（Markdown 预览、编辑器高亮、`StyledText`）
- **mdict 格式分析** — 搜索 "MDX file format analysis" / "mdict 格式解析"，格式已有充分逆向文档
- **lightningcss 文档** — CSS 解析与 AST 遍历

> 📌 上述链接来自检索结果，未逐一验证可用性与最新状态，实施前请自行确认。

---

## 14. 关键决策摘要

1. **不做浏览器，做语义提取器。** HTML → AST → gpui，而非 HTML → 像素。
2. **颜色/字体语义化。** 这是主题切换与暗色模式的前提。
3. **内联富文本合并为单次绘制。** 这是性能的关键。
4. **AST 磁盘缓存。** 这是查词延迟的关键。
5. **按需读取 + 虚拟滚动。** 这是内存的关键。
6. **放弃 100% 还原，追求更好的排版。** 这是心态的关键。
7. **Phase 0 先验证 gpui 能力。** 这是风险控制的关键。

---

*文档结束*
