# CLAUDE.md

Guidance for working on MoeDict — a lightweight, cross-platform MDX desktop dictionary.

## What this is

MoeDict is a Tauri 2 desktop app. The dictionary engine lives in a UI-agnostic
Rust crate; the frontend is plain HTML/CSS/vanilla JS embedded into the Tauri
binary (no npm, no bundler). The old axum web server is kept in `legacy/` and is
**excluded from the workspace**; do not edit or build it unless the user asks.

- `crates/mdict-core` — MDX/MDD parsing, immutable index, dictionary registry,
  query/suggestions/resources.
- `src-tauri` — Tauri shell, IPC commands, background index worker,
  `mdict://` resource protocol.
- `ui` — static frontend, embedded at compile time.
- `resources/mdx` — sample dictionaries; only `folkets_sv_en.mdx` is tracked, all
  other dictionary data is gitignored.

## Commands

```bash
cargo run -p moedict                      # build + launch
cargo run -p moedict -- --mdx-dir ./resources/mdx
cargo test -p mdict-core                  # fast unit + sample e2e tests
cargo fmt                                 # rustfmt config at repo root
cargo check -p moedict                    # needs WebKitGTK/GTK dev packages on Linux

# diagnostics
cargo run --release -p mdict-core --example inspect -- <file.mdx|mdd> [--limit N]
cargo run --release -p mdict-core --example e2e -- <file.mdx> <word>
```

Bundling: `cargo install tauri-cli --version "^2" --locked` then
`cargo tauri build` (outputs `target/release/bundle/`).

## Architecture

### Parsing (`crates/mdict-core/src/mdict/`)

- `header.rs` — big-endian length + UTF-16LE XML header; checksum is LE Adler-32.
  `read_title()` only reads the header.
- `mdict.rs` — memory-maps the file; parses key blocks and the record-block
  size table. Record blocks are decompressed **once per block**, not per entry.
  Key-info text lengths are **bytes for single-byte encodings** and **UTF-16
  code units** for UTF-16/MDD (important: MDD keys are UTF-16LE).
- `cursor.rs` — bounds-checked binary reader.
- Supported: v1 best-effort, v2; encryption 0/1/2/3; compression none/LZO/zlib.

### Index (`crates/mdict-core/src/index/`)

Custom immutable `.idx` format, no SQL engine:

```
[128B header][32B records * N][key pool][zstd blobs]
record = key_off u64 | key_len u32 | def_off u64 | def_len u32 | raw_len u32 | flags u32
```

- Keys are sorted by `trim().trim_start_matches('\u{feff}').to_lowercase()`.
- Definitions are zstd level 3 when that is smaller; otherwise raw.
- Built into a temp file, fsynced, atomically renamed (`replace_file` retries on
  Windows while a query may still map the old file).
- `IndexFile` mmaps the file and answers exact/prefix/random lookups. `exact`
  prefers a case-exact match, then falls back to the first case-insensitive
  match; it also follows `@@@LINK=target` redirect entries (depth-limited,
  cycle-safe, fragment stripped) and keeps the queried key as the headword.
- Bump `FORMAT_VERSION` when changing the layout; stale versions are rebuilt.

### Registry (`crates/mdict-core/src/registry.rs`)

`DictionaryStore` owns `config.json`, the index directory and LRU caches:

- add files/folders (recursive, max depth 8/5000 files), canonical paths,
  `ripemd256` id from the display path;
- enable/rename/reorder/remove; orders are normalised to 0..n;
- `build_index` streams entries into `IndexBuilder`, verifies the source
  size/mtime did not change, saves metadata and clears `last_error`;
- status is computed against the **current file metadata**, so external edits
  show as `stale`;
- `query` returns hits from every enabled dictionary in user order;
  `suggest` merges prefixes; `lucky` picks a plausible random headword;
- `read_resource` resolves external files inside the dictionary directory
  first, then MDD resources (lower-cased keys, path variants, split MDDs).

Config is written atomically (`config.json.tmp` + rename). Temp files older than
6 hours are cleaned at startup.

### Tauri (`src-tauri/src/`)

- `lib.rs` — plugins (single-instance first, then log/dialog/opener/window
  state), custom protocol registration, state setup, optional `--mdx-dir`.
- `commands.rs` — IPC surface. Tauri converts snake_case Rust args to camelCase
  JS keys. File dialogs use the blocking plugin API inside `spawn_blocking`.
- `indexer.rs` — a single background thread with an mpsc queue. Emits
  `index-progress`, `index-done`, `dict-changed`. Cancellation uses
  `AtomicBool`; progress is throttled to ~80ms.
- `protocol.rs` — `mdict://localhost/<dict-id>/<resource>` on Linux/macOS and
  `http://mdict.localhost/<dict-id>/<resource>` on Windows. Adds CORS and
  cache headers; 404/500 otherwise.

### Frontend (`ui/`)

No bundler and no framework. `window.__TAURI__` is available because
`app.withGlobalTauri = true`.

- `app.js` — state, search/suggestions, dictionary CRUD UI, drag reorder,
  settings/rename/remove modals, context menus, toasts, lazy iframes.
- Rendering: each hit is sanitized with `DOMParser`, wrapped in a full HTML
  document and put in an `<iframe sandbox="allow-same-origin">` (scripts cannot
  run). CSS/JS/URL rewriting:
  - `entry://` and bare links → `data-entry` (search internally), with the
    `#fragment` kept as `data-entry-anchor`;
  - `#fragment` anchors (including percent-encoded `entry://%23anchor`, and
    `<area>` image maps) → `data-anchor`, scrolled by the host; `href="#"`/
    unknown anchors mean the top of the entry. Decode before splitting on `#`
    (OLCC writes `entry://%23_hkn3` for `entry://#_hkn3`). `srcdoc` frames
    inherit the app's base URL, so any unhandled fragment/relative navigation
    would load the app page inside the entry frame — always `preventDefault`;
  - `sound://` → `data-sound` (audio playback via fetch + Blob);
  - `http(s)://` → `data-external` (OS opener, only http/https/mailto);
  - relative/`file:///` → `mdict://` resource URLs;
  - `<script>`, event handlers, forms, `javascript:` URLs removed;
  - remote stylesheets/images are left for CSP to block.
- `xlink:href` cannot be used in a CSS selector — handle it manually.
- Iframes are loaded lazily with `IntersectionObserver`; heights track a
  `ResizeObserver`; entries taller than 1500px get a "展开全文" control.
- Result-pane jumps (sidebar/nav chips and entry anchors) go through
  `scrollResultsTo`, which caps the animation at a fixed ~260ms. Do not put
  `scroll-behavior: smooth` on `.results`: the browser's distance-based
  animation takes seconds on long entries.
- Dictionary order matters for result order. Filtering the sidebar disables
  drag reorder.
- UI state (history) lives in `localStorage`; settings live in the core config.

### Security / CSP

Configured in `src-tauri/tauri.conf.json`. Keep `mdict:` **and**
`http://mdict.localhost` in `style-src`/`img-src`/`media-src`/`font-src`/
`connect-src`; keep `frame-src 'self' data: about:` for `srcdoc` iframes.
`script-src 'self'` only (Tauri adds nonces for its own scripts).

## Platform notes

- Linux build needs `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf`;
  Chinese text needs a CJK font (`fonts-noto-cjk`).
- Windows needs the WebView2 runtime and MSVC build tools.
- `cfg!(windows)` selects the custom-protocol base URL.
- The local toolchain is Rust 1.94; Tauri 2.12 declares MSRV 1.90. Avoid deps
  that require newer releases.

## Conventions

- User-facing strings are Chinese; code comments/doc comments are English.
- Prefer adding functionality to `mdict-core` and keeping the Tauri layer thin.
- Do not reintroduce SQLite or a web server. If full-text search is needed,
  add it as an optional sidecar rather than replacing the read-only index.
- `cargo fmt` before finishing. Keep the UI dependency-free.
