# WebView2 内存占用优化记录

> 日期：2026-11-26 · 分支：`main` · 相关配置：`src-tauri/tauri.conf.json`

## 问题与结论

现象：`MoeDict-0.1.0-portable.exe` 本体很小，但运行时 WebView2 占用约 300–450MB。

结论：**不是每本词典一个 WebView**。Tauri 配置只有一个窗口，Wry/WebView2 只有一个浏览器实例；
每本词典的释义是同一 WebView 内的 `<iframe srcdoc sandbox="allow-same-origin">`。
Chromium 自身会派生浏览器、GPU、network、crashpad、renderer 等进程，默认共 6 个 WebView2 子进程 +
1 个主进程（实测 7 个进程）。这部分是固定基线，不是按词典数量线性增长的。

## 测量方法

Windows 11，用户配置目录下 17/28 本词典启用，查询 `book`（9 本命中），
再通过 `PageDown` 滚动到结果列表底部，使所有结果卡片均进入过加载范围。

采集方式：PowerShell `Get-CimInstance Win32_Process` 按父进程链汇总 MoeDict 主进程及其 WebView2 后代，
记录 `WorkingSet64`（工作集）与 `PrivateMemorySize64`（私有内存）。
工作集波动较大，以私有内存为主要比较指标。

## A/B 结果

| 方案 | 进程数 | 工作集 | 私有内存 | 结果 |
|---|---:|---:|---:|---|
| 原始配置（旧 UI） | 7 | 450.1 MB | 355.8 MB | 基线 |
| iframe 卸载上限 3 个 + rootMargin 420→200 | 7 | 480.6 MB | 406.1 MB | 反而升高，已撤销 |
| `--renderer-process-limit=1 --disable-features=...,site-per-process --js-flags=--max-old-space-size=128` | 7 | 499.2 MB | 415.3 MB | 参数生效但无收益，已撤销 |
| `--single-process --no-zygote`（保留 wry 默认 disable-features） | 3 | 321.6 MB | 297.8 MB | **采用** |

`--single-process` 版本的命令行已通过 WMI 确认包含该参数；截图确认查询、滚动、结果导航、释义渲染均正常。

## 采用的改动

`src-tauri/tauri.conf.json` → `app.windows[0].additionalBrowserArgs`：

```
--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --single-process --no-zygote
```

注意：设置 `additionalBrowserArgs` 会**覆盖** wry 的默认参数，必须把默认的
`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection` 写回去，否则会重新出现迷你菜单/SmartScreen 弹窗。

## 风险与后续追因

- `--single-process` 取消进程隔离：renderer 崩溃可能导致整个 WebView 一起挂掉，需要长期运行观察。
  若出现稳定性问题，直接删除 `additionalBrowserArgs` 即可回退到基线。
- 去掉 `--js-flags=--max-old-space-size=128`：大词典（如 92k 词条的牛津8）下 128MB 堆有 OOM 风险，
  实测收益也来自 `--single-process` 而非堆上限。
- iframe 卸载方案无效的可能原因：同源 iframe 共用 renderer，卸载/重建只增加分配开销，不会减少 Chromium 固定进程。
- 若要进一步大幅降低内存，只能脱离 WebView2（见 `feat/gpui-native` 分支的原生渲染实验），
  该分支当前已封存，不作为当前交付方案。
- 复测时务必使用同一组词典、同一查询词和相同的滚动步骤，否则工作集差异可达数十 MB。
