# 校园通知 · 日程 —— WinUI 3 原生桌面客户端

> 子项目说明。仓库**根目录**的 `README.md` 描述现有 Node 服务 + 原生 JS 网页版，仍然准确，本文件不覆盖它。

## 1. 定位

`CampusNotice` 是一个 **WinUI 3 / Windows App SDK 原生 XAML 桌面客户端**，把现有 Web 版的四页体验搬到 Windows 原生界面，并补上三项桌面增强（系统托盘 / 任务栏跳转列表 / 原生 Toast）。

**它不重新实现任何业务逻辑**：抓取、评分、AI 摘要、CAS 自动登录、日程与提醒、JSON 落盘全部继续由仓库根目录的
`server.mjs` + `store.mjs` + `core.mjs` + `llm.mjs` + `login.mjs` + `notify.mjs` 承担。客户端只通过本地 HTTP 与后端通信：

```
CampusNotice.exe  ──GET  /api/state────────┐
                  ──POST /api/action──────┤  127.0.0.1:8737
                  ──GET  /api/health──────┤  server.mjs (Node v22)
                  ──POST /api/shutdown────┘
                  ──GET  /rss.xml（只读展示地址）
```

客户端启动时先探测 `/api/health`：已有健康实例（Web 版、`start.cmd`、另一客户端）就**复用，不再 spawn**；否则以
`node server.mjs --no-toast --no-open` 隐藏窗口自动托管，遵循「谁启动谁关闭」——只有本实例启动的进程才在退出时收到 `/api/shutdown`。

## 2. 与现有后端的唯一改动

`server.mjs` 只做了 **2 处最小改动**（默认行为完全不变）：

1. `parseArgs()` 增加 `--no-toast` 参数，默认 `false`；
2. `onNotify` 回调在该参数为 `true` 时只写日志并 `return true`，不调用 `toast()`。

原因：`store.mjs` `recordReminder()` 是**先写 `meta.reminders` 再判断 `settings.notify`**，所以后端静默化不会丢提醒记录；客户端改由 `AppNotification` 单一通道消费 `meta.reminders` 发系统 Toast。
`start.cmd` / Web 版不带该参数，零影响。详见 `docs/winui3-build-and-run.md` 的「唯一后端改动与回滚」。

**未改动**：`public/`、`core.mjs`、`store.mjs`、`llm.mjs`、`login.mjs`、`notify.mjs`、根目录 `README.md`。

## 3. 依赖

| 项 | 版本 / 说明 |
| --- | --- |
| 目标框架 | `net8.0-windows10.0.19041.0`（`TargetPlatformMinVersion 10.0.17763.0`） |
| Windows App SDK | `Microsoft.WindowsAppSDK`（csproj 里写的是 `1.6.240923002`；**按 VS 中实际安装的版本调整**）。用到了 `MicaBackdrop`、`AppNotificationManager`、`InfoBadge`、`Expander`、`SelectorBar` 级别的 API 面，要求 ≥ 1.5，推荐 1.6 |
| 其它 NuGet | 无。未引入 CommunityToolkit（筛选改用 3 个 `ToggleButton`，视觉规范 §7.5 明确允许的备选方案）；`Microsoft.Windows.SDK.BuildTools` 由 WindowsAppSDK 传递引用 |
| Node.js | ≥ 18（本机实测 v22.22.2），仅运行期需要，不需要打包进应用 |
| 编译工具 | Visual Studio 2022 + 「.NET 桌面开发」+「Windows 应用 SDK」工作负载 + Windows 11 SDK (10.0.22621 或更高) |

## 4. 目录结构

```
winui3/
├─ CampusNotice.sln                 用 VS2022 打开这个
├─ README.md                        本文件
└─ CampusNotice/
   ├─ CampusNotice.csproj           net8.0-windows10.0.19041.0 / WindowsPackageType=None
   ├─ Package.appxmanifest          MSIX 打包用（unpackaged 时不生效）
   ├─ app.manifest                  Per-Monitor v2 DPI、asInvoker（不提权）
   ├─ App.xaml(.cs)                 启动 / 单实例 / Mica / AppNotification 注册与激活路由
   ├─ MainWindow.xaml(.cs)          NavigationView 四页外壳、全局命令栏、单一 InfoBar、状态行、三档断点
   ├─ Themes/                       Colors / Brushes / Typography / Spacing / IconGlyphs（视觉规范 §10 Token 表）
   ├─ Models/                       System.Text.Json 契约映射 + 视图模型
   ├─ Services/                     NodeBackendHost / ApiClient / StateStore / ToastService / TrayIconService / JumpListService
   ├─ Views/                        NoticesPage / SchedulePage / SourcesPage / SettingsPage
   ├─ Controls/                     ScoreBadge / NoticeCard / SourceEditor
   ├─ Helpers/                      Converters / Dialogs / ScorePalette / Formatters / Icons / LocalPrefs
   └─ Assets/README.md              图标规格与三种可执行的转换方案
```

## 5. 如何打开 / 构建 / 运行

1. 双击或在 VS2022 里打开 `winui3/CampusNotice.sln`；
2. 首次打开让 NuGet 还原；若报 WindowsAppSDK 版本不匹配，把 `CampusNotice.csproj` 里的 `Version` 改成 VS 实际安装的版本；
3. 选择 `x64` + `Debug`，`生成 → 生成解决方案`；
4. `调试 → 开始调试`（或按 F5）。默认 `WindowsPackageType=None`，**unpackaged 直跑**，不需要签名、不需要打包、不需要管理员权限；
5. 应用会自动探测 / 托管 `server.mjs`。**必须能从 exe 所在目录向上找到仓库根目录下的 `server.mjs`**（`NodeBackendHost.ResolveBackendRoot()` 会从 `AppContext.BaseDirectory` 逐级向上找，也支持环境变量 `CAMPUS_NOTICE_ROOT` 显式指定）。用 VS 从本解决方案启动即可满足；若把 exe 单独拷到别处，请设置该环境变量。

> 想启用任务栏跳转列表（JumpList）需要 MSIX 包标识，见 `docs/winui3-build-and-run.md`。

## 6. 首次运行前必须补齐的二进制图标

`Assets/` 目录**当前只有 `README.md`，没有任何 `.ico` / `.png`**。这是有意的：本机没有 Windows SDK 头文件 / VS2022 / 图像工具链，无法产出经过验证的图标资源。为了不让 csproj 因引用不存在的图标而编译失败：

- csproj **没有**设置 `<ApplicationIcon>`；
- `Package.appxmanifest` 里的 `Logo` / `Square44x44Logo` 等只是 MSIX 打包时的占位路径；
- `TrayIconService` 找不到 `.ico` 时回退 `LoadIcon(NULL, IDI_APPLICATION)`，不会崩溃；
- NavigationView 品牌头用 `FontIcon`（`IconRinger`）而非位图。

**首次构建前请按 `CampusNotice/Assets/README.md` 补齐** `AppIcon.ico`（16/20/24/32/48/256）、`TrayLight.ico` / `TrayDark.ico`（深色单色白版）以及 MSIX 用的 PNG，并按需把 `<ApplicationIcon>` 加回 csproj。图标码位也需要在落地前按 Segoe Fluent Icons 字形表核对一次（见 `Themes/IconGlyphs.xaml` 顶部说明）。

## 7. 三处关键的、与 Web 版有意不同的决策

| # | 决策 | 落地 |
| --- | --- | --- |
| D1 | 「重要」筛选阈值 **`score >= 50`**（Web 版 `app.js` L231/L234 是 55） | `ScorePalette.ImportantThreshold = 50` 是唯一判定入口；筛选按钮 ToolTip / HelpText、激活时的常驻说明行、徽标「{档位} {score}」+ 形状三重编码 |
| D2 | `loginExpired` = **Warning 橙 + 必有「重新登录」**，红色只给断连 / 整体抓取失败 | `SourceLoginExpiredBrush` 为橙色；`account.test`（已配置）/ 跳设置页 `account.save`（未配置）；全局只出 Warning InfoBar |
| D3 | 原生 Toast = 客户端 `AppNotification` **单一通道** | 后端 `--no-toast`；客户端唯一 10s 轮询 `meta.reminders` 按 `reminder.id` 去重，只对会话启动后的新 id 发；`settings.notify=false` 时不弹；「测试桌面通知」走客户端自测 Toast，**绝不调用 `notify.test`** |

## 8. 零开机自启

设置页、托盘菜单、跳转列表、首次运行引导四处**没有任何启动类入口**；不写启动文件夹、不写注册表启动项、不建计划任务。四处文本/资源检索命中数为 0。
「到点弹桌面通知」只是提醒通道开关，「关闭窗口后可在托盘找到」只描述关闭后的行为，都不涉及启动行为。

## 9. 诚实性声明（重要）

> **本机未编译验证。**

> **规范版本说明**：本实现已完成对 **视觉规范 v1.1 §13（权威修订）/ 体验规范 v1.1 同步 S1–S5** 的对齐——
> 包括底部居中 8s 撤销条（取代 TeachingTip 撤销）、`item.add` 改走 Success InfoBar、
> 复用健康服务不显示启动态、4 个行为开关的 14px ProgressRing 在途忙态、
> 「重要 = 关注度 ≥ 50 的通知」12px Caption 落点。逐项登记见 `docs/winui3-engineering.md` 第 9 节。
> **轮询统一为唯一 10s 一次**（主理人最终裁定）：同时承担 UI 静默刷新与 `meta.reminders`
> 增量检测，不另起提醒轮询；后端 `checkReminders` 每 15s 独立运行属**服务端事实**，
> `reminder.id` 持久且 state 保留前 20 条，不会漏提醒。规范与实现**无冲突**。

本机实测只有 .NET 8/9/10 **运行时**与 Node v22.22.2，**没有 .NET SDK、没有 VS2022、没有 Windows SDK 头文件**，因此本解决方案**无法在本机构建或运行**。交付前做了以下静态自检（结果见 `docs/winui3-engineering.md`）：

- 所有 `op` 字符串逐一与 `server.mjs` / `store.mjs` 真实契约比对（23 个全部命中，无虚构 API）；
- 用 Node 实际起了一个隔离的 `server.mjs --no-toast --no-open`（临时数据目录、临时端口），验证 `/api/health`、`/api/state` 字段形状，以及**所有 `/api/action` 响应（含 `ok:false`）都回传全量 `state`**；
- `node --check server.mjs` 通过；所有 `.xaml` XML 良构；
- 检查 XAML 引用的 `ThemeResource` / `StaticResource` 键在 `Themes/` 或 WinUI 系统资源中有定义（0 处未解析）；`Colors.xaml` 的 Light / Dark / HighContrast 三分支键集合完全一致（92/92/92）；
- 检查所有 XAML 事件处理器在对应 code-behind 中存在（0 处缺失）；
- 检查 csproj 框架 / 包引用自洽。

**首次构建请在装有 VS2022 + .NET 桌面开发 + Windows 应用 SDK 工作负载的机器上进行**，并预留时间处理 WindowsAppSDK 版本、图标资源与 HighContrast 分支的运行时验证。已知限制与未验证项完整清单见 `docs/winui3-engineering.md` 与 `docs/winui3-build-and-run.md`。

## 10. 相关文档

- `docs/winui3-visual-spec.md` — Phase 1 视觉规范（12 节，§10 Token 表）
- `docs/winui3-ux-spec.md` — Phase 2 交互 / 响应式 / 无障碍规范（§10.9 逐页 op 映射与验收口径）
- `docs/winui3-engineering.md` — 架构、文件职责、op × 页面映射、性能、已知限制
- `docs/winui3-build-and-run.md` — 环境、构建、unpackaged / MSIX、图标、故障排查、后端改动回滚
