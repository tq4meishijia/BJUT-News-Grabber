# WinUI 3 客户端 · 工程说明（Phase 3）

> 对应 `winui3/CampusNotice.sln`。上游依据：`docs/winui3-visual-spec.md`（§10 Token 表）、
> `docs/winui3-ux-spec.md`（§10.9 逐页 op 映射）；后端契约：`server.mjs` / `store.mjs` / `core.mjs`。

---

## 0. 一句话架构

**一个进程、一个窗口、一个 `StateStore`、一个后端 `server.mjs`。**
所有写操作走 `POST /api/action`，所有 op 的响应（**含 `ok:false`**）都回传全量 `state`；客户端不做本地乐观数据模型，统一用返回的 `state` 整体刷新。UI 只是 `state` 的渲染器 + 命令发射器。

```
┌──────────────────────────── CampusNotice.exe (WinUI 3, unpackaged) ───────────────────────────┐
│ App (单实例 / Mica / AppNotification 注册 / 激活路由)                                        │
│  └ MainWindow (NavigationView 四页外壳 / 全局命令栏 / 单一 InfoBar / 状态行 / 3 档断点)     │
│     ├ NoticesPage ┐                                                                          │
│     ├ SchedulePage├─ 只实现 IPageView：ApplyState / HandleCommand / ApplyCompact / OnPageActivated
│     ├ SourcesPage │        (+ LocateItem / LocateByNoticeId / ApplyFilter)                   │
│     └ SettingsPage┘                                                                          │
│  ── App.State (StateStore) ── 10s 轮询 GET /api/state + 写操作串行队列 + 单调版本号         │
│  ── App.Api   (ApiClient)  ── HttpClient，默认不带 Origin（server.mjs sameOrigin 校验通过） │
│  ── App.Host  (NodeBackendHost) ── health 探测 → spawn --no-toast --no-open → /api/shutdown │
│  ── App.Toast (ToastService) ── AppNotification 单一通道，reminder.id 去重                 │
│  ── App.Tray  (TrayIconService) ── Win32 Shell_NotifyIcon + 系统菜单                       │
│  ── App.JumpList (JumpListService) ── 3 项「常用」（需包标识）                             │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
                          ↕ 127.0.0.1:8737  (GET /api/state, POST /api/action,
                                              GET /api/health, POST /api/shutdown, GET /rss.xml)
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ server.mjs (Node v22) → store.mjs → core.mjs / llm.mjs / login.mjs / notify.mjs             │
│ 数据：%APPDATA%\campus-notice\store.json + config.json（CAS 密码 DPAPI 加密）               │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

### 三条硬原则

1. **单一数据源（A2）**：`StateStore.Current` 是唯一真值。`GET /api/state` 只用于启动与 10s 静默刷新；
   `POST /api/action` 成功后**和失败后**都应用 `response.State`；只有请求异常（无 state）才走「服务断连」流程。
2. **谁启动谁关闭（A3）**：启动先 `GET /api/health`，健康就复用（不 spawn，避免 `EADDRINUSE`）；
   只有本实例 spawn 了 node 才在退出时发 `/api/shutdown`。
3. **四页平级、无返回栈**：4 个 `Page` 实例只创建一次，用 `Visibility` 切换，不建 `Frame` 返回栈；
   切页保留筛选与滚动位置（`LocalPrefs` 持久化筛选 / 显示已完成 / 上次所在页）。

---

## 1. 文件职责表

| 文件 | 职责 |
| --- | --- |
| `App.xaml(.cs)` | 合并 5 个 Theme 字典；`AppInstance.FindOrRegisterForKey` 单实例；装配服务；`StartBackendAsync` / `RetryConnectionAsync`；`AppInstance.Activated` 与 AppNotification 冷启动激活路由；静态服务定位器 `App.Api/State/Host/Toast/Tray/JumpList/Window/Status` |
| `MainWindow.xaml(.cs)` | NavigationView 四页外壳；品牌头与 PaneFooter 状态摘要；顶部全局 CommandBar；**唯一** `InfoBar` + 状态行；2px 抓取进度条；后端启动中央 ProgressRing；Mica/HighContrast backdrop；三档 DIP 断点；Alt+1..4 / F5 / Ctrl+R / Ctrl+S / Esc 快捷键；关窗=Hide 到托盘、退出才发 shutdown；**底部 8s 撤销条**（组件 7.14，独立于顶部 InfoBar）；统一 `TeachingTip`（首次引导 / 关闭到托盘提示 / JSON 一键切换）；跨页待撤销集合与退出前 flush；首运行 3 步引导 |
| `Themes/Colors.xaml` | 视觉规范 §10.1 颜色 Token，Light / Dark / HighContrast 三套 `ThemeDictionaries`，各 92 键完全一致。**唯一允许出现裸 HEX 的文件** |
| `Themes/Brushes.xaml` | 语义别名 + `ThemeShadow`（Z=32 / Z=64）+ 卡片 / chip / 正文面板 `Style`（不透明实色 + 1px 描边 + 圆角 8 + 零投影） |
| `Themes/Typography.xaml` | §10.2 字号阶梯（28/20/14/12/11 DIP，显式中文行高 36/28/20/16/14）；数字采用「固定宽度容器 + 右对齐 + `CharacterSpacing=0`」的 tabular 退化方案 |
| `Themes/Spacing.xaml` | §10.3 间距 / 圆角 / `MinHeight` / 最大宽 940·720 / 断点 1008·640 / 图标尺寸 / 浮层 Z |
| `Themes/IconGlyphs.xaml` | §10.4 图标 `x:String` 常量（XML 数字字符引用）；注释中明确要求落地前核对 Segoe Fluent Icons 字形表 |
| `Models/AppState.cs` | `state` 根对象 + 派生计数（未读 / 重要≥50 / 待办 / loginExpired / error 源） |
| `Models/Notice.cs` | `state.notices[]`，对齐 `store.mjs` L429-450 与 `app.js` 渲染字段 |
| `Models/ScheduleItem.cs` | `state.items[]`，对齐 `makeItem()` L579-597（含 `fired[]`、`doneAt`、`noticeId`） |
| `Models/SourceItem.cs` | `state.sources[]`，对齐 `state()` L977-999（`map` 用 `JsonElement?` 避免任意 JSON 反序列化失败） |
| `Models/Settings.cs` | `state.settings`；`TogglePatch()` 生成行为开关的单键 patch |
| `Models/Meta.cs` | `state.meta`（`reminders[]` / `llm` / `account` / `polling` / `dataPath` / `port` / `llmKeySet`） |
| `Models/Reminder.cs` | `recordReminder()` L627-641 的 `{id,itemId,title,note,dueAt,at}` |
| `Models/ActionResponse.cs` | `{ok,message?,error?,preview?,id?,state}` + `StateEnvelope` / `HealthResponse` / `ShutdownResponse` / `PreviewItem` |
| `Models/ViewModels.cs` | `ObservableObject` 基类 + `NoticeViewModel` / `ScheduleGroupViewModel` / `ScheduleItemViewModel` / `SourceViewModel` / `GlobalStatus`。**就地更新属性**而不是重建对象，保焦点与滚动 |
| `Services/ApiClient.cs` | `Ops` 常量表（23 个真实 op）+ `HttpClient` 封装；统一超时与 `ApiException`；默认不带 `Origin` |
| `Services/StateStore.cs` | 单一数据源；唯一 10s 轮询；写队列串行 + 在途暂停静默刷新 + 单调版本号；`StateChanged` / `ConnectionChanged` |
| `Services/NodeBackendHost.cs` | `ProbeHealthAsync()` 先探活（复用路径不显示启动态）→ spawn `node server.mjs --no-toast --no-open` → 400ms 轮询、15s 超时 → `ShutdownAsync()`（仅 OwnsServer）；`ResolveBackendRoot()` 向上找 `server.mjs` / `CAMPUS_NOTICE_ROOT` |
| `Services/ToastService.cs` | `AppNotificationManager` 注册 / 反注册；`ProcessReminders()` 按 `reminder.id` 去重、只对会话启动后新 id 发 Toast；`settings.notify=false` 双端尊重；`ShowTestNotification()` 客户端自测；「打开日程」激活 |
| `Services/TrayIconService.cs` | Win32 `Shell_NotifyIcon` + 专用 STA 消息循环 + `TrackPopupMenuEx` 系统菜单（8 项）；菜单文案/禁用态由 `Update()` 从 UI 线程刷新 |
| `Services/JumpListService.cs` | `Windows.UI.StartScreen.JumpList`「常用」3 项；unpackaged 无包标识时静默降级 |
| `Views/NoticesPage` | 三态筛选 / 批量已读+撤销条 / AI 重摘要 / 一键加日程(Success InfoBar) / 8s 撤销条移除 / 打开原文 / 空态 / 200 上限提示；「重要 = 关注度 ≥ 50 的通知」12px Caption |
| `Views/SchedulePage` | 六(+已完成)分组 / 手动新增(零提示) / 改时间 / 重置提醒 / 完成态 / 删除+撤销条 / 深链定位 |
| `Views/SourcesPage` | 启用停用 / type 显隐矩阵 / 新增 / 内联编辑 / 测试 preview / 重新登录落点 / 删除二次确认 / 底部说明 |
| `Views/SettingsPage` | 五分组 / 表单显式一次性保存 / 4 开关防抖即时保存 / API Key + CAS(PasswordBox) / 运行状态 / `notices.rescore`·`notices.refill` / 客户端 Toast 自测 |
| `Controls/ScoreBadge` | 「档位文字 + score 数字 + 形状」三重编码；4 档形状由 `ScorePalette` 决定；`AutomationProperties.Name` 完整 |
| `Controls/NoticeCard` | 通知卡片；静态 `Command` 事件把 6 类交互交给 `NoticesPage` 统一映射 op；More Flyout 动态构建用 `Tag` 携带 id |
| `Controls/SourceEditor` | 新增 / 编辑共用的字段表单，显隐由 `SourceViewModel.Type` 驱动；Cookie `PasswordBox` 用 `PasswordChanged` 写回草稿，绝不回显 |
| `Helpers/ScorePalette.cs` | 四档阈值 70/50/35、筛选阈值 50（唯一判定入口）、形状枚举、`ThemeBrush.Get()` 按主题从 ThemeDictionaries 精确取色 |
| `Helpers/Formatters.cs` | 状态行 / `dayLabel()` / 倒计时 / 提醒档位 / 24h 到期数 / 今日到期数 / 浏览器打开 / 时间归一化 |
| `Helpers/Converters.cs` / `Dialogs.cs` | XAML 转换器；破坏性 `ContentDialog`（`DefaultButton = Close`） |
| `Helpers/Icons.cs` / `LocalPrefs.cs` | code-behind 图标常量；`%APPDATA%\campus-notice\winui3-prefs.json` 本地偏好（首次引导标记 / 筛选 / 上次页面） |

---

## 2. op × 页面映射表（静态逐一比对）

> `Ops` 表全部取自 `store.mjs` `act()` L646-969 与 `server.mjs` L241-310。**无虚构 API**。
> 「回传 state」列：✅ = 后端确认返回全量 `state`，包括 `ok:false`（已用 Node 起隔离实例实测）。

| op | 后端位置 | 页面 / 入口 | 载荷 | 回传 state | 客户端处理 |
| --- | --- | --- | --- | --- | --- |
| `poll` | `store.mjs` L651 | 通知/日程/数据源命令栏、托盘第 3 项、跳转列表「立即抓取」、首次引导第 3 步 | `{}` | ✅ | 本地 loading；完成用 `notices` 数量差算「新增 N 条」；「没有启用任何数据源」→ Warning；`polling` 期间点击直接忽略 |
| `notice.read` | L655 | 通知卡 More「标已读/标未读」、点击标题打开原文、`readAll` 撤销 | `{id,read}` | ✅ | 静默；卡字重/圆点/竖条变化，未读徽标更新 |
| `notice.readAll` | L661 | 通知筛选行右端 | `{}` | ✅ | 执行前缓存未读 id 集合，撤销时逐条 `notice.read{read:false}` |
| `notice.remove` | L666 | 通知卡 More「移除」 | `{id}` | ✅（延迟提交后才发） | **8s 底部撤销条 + 延迟提交**；撤销窗内零写请求；多条合并为「已移除 N 项」 |
| `notice.summarize` | L671 | 通知卡 More「AI 重摘要」 | `{id}` | ✅（失败也刷新） | 失败保留原摘要/score，只在卡内联显示原因 + 可重试，**不升级全局 Error** |
| `notices.rescore` | L695 | 命令栏溢出、设置运行状态区 | `{}` | ✅ | 展示后端 `message`，注明「AI 摘要保留」 |
| `notices.refill` | L720 | 命令栏溢出、设置运行状态区 | `{limit:40}` | ✅ | 按钮 loading + 结果 `message` |
| `item.add` | L771 | 通知「加入日程」、日程手动新增 | `{title,note,dueAt,remindBefore?,source,noticeId,url}` | ✅（返回 `id`） | v1.1 §13.3 方案 1：**不给撤销条 / 不给「查看日程」按钮**，只用 Success InfoBar + 返回 state 刷新；返回的 `id` 仍是后端真值，反悔时到日程页删除（删除走撤销条）；无截止时间提交前用 TeachingTip 提示兜底明天 09:00 |
| `item.update` | L788 | 日程行内联「改时间」 | `{id,title?,note?,dueAt?,remindBefore?}`（只传变化字段） | ✅ | 编辑态明示「改时间/提醒档位会清空 `fired[]`」 |
| `item.done` | L807 | 日程行 CheckBox | `{id,done}` | ✅ | 可反向取消；完成态=删除线+「已完成」文字+Name |
| `item.remove` | L815 | 日程 More「删除」 | `{id}` | ✅ | 日程删除同样走 8s 撤销条 + 延迟提交 |
| `item.refire` | L820 | 日程 More「重置提醒」 | `{id}` | ✅ | 仅 `fired.length>0` 时出现 |
| `source.add` | L828 | 数据源内联新增表单 | `{name,type,url,enabled,cookie,resolveIp,linkPattern,itemsPath,pages,pageUrl,urlTemplate,detailUrlTemplate,contentPath,map,note}` | ✅ | type 只给 html/rss/json；url 非 http(s) 前置拦截；按 type 只提交可见字段 |
| `source.update` | L867 | 启用开关（即时）、内联编辑 | `{id,enabled}` 或变化字段 | ✅ | 开关在途禁用、失败由返回 state 回滚；Cookie 留空不修改 |
| `source.remove` | L892 | 数据源「删除」 | `{id}` | ✅ | **ContentDialog 二次确认**（`DefaultButton=Close`） |
| `source.test` | L897 | 数据源「测试」 | `{id}` | ✅（失败也刷新） | 成功内联 ≤5 条 `preview[]`；失败内联原因；命中「改成 JSON」提示给一键切换；按钮 loading 防并发 |
| `settings.save` | L940 | 设置分组 1 显式保存、4 行为开关、通知页「开启桌面通知」 | `{settings:{...}}` | ✅ | 表单一次点击只发 1 次；开关防抖 400ms + 串行 + 在途禁用（因 `restartTimers()`） |
| `llm.key` | `server.mjs` L241 | 设置「保存 Key」/「清空 Key」 | `{apiKey}` | ✅ | `PasswordBox` 不回显；空输入不直接提交；清空走 ContentDialog |
| `llm.test` | L246 | 设置「测试模型」 | `{}` | ✅ | 失败不覆盖已存 Key、不升级全局 Error |
| `account.save` | L256 | 设置「保存并登录」、未配置时的「重新登录」落点 | `{username,password,casBase,service}` | ✅ | 4 字段前置校验；`PasswordBox`；后端立即 `autoLogin()` |
| `account.test` | L292 | 设置「测试自动登录」、数据源/全局「重新登录」（`configured=true`） | `{}` | ✅ | 失败保留 Warning + 「去设置页填写账号」 |
| `account.clear` | L301 | 设置「清除已保存」 | `{}` | ✅ | ContentDialog 二次确认，清空 4 个输入框草稿 |
| `notify.test` | `store.mjs` L962 / `server.mjs` L307 | **客户端不调用** | — | ✅ | 决策 3：设置/命令栏「测试桌面通知」一律走 `ToastService.ShowTestNotification()` |

**端点**：`GET /api/state`（启动 + 唯一 10s 轮询）、`GET /api/health`（启动探测 + 托盘标题）、
`POST /api/shutdown`（仅退出且 OwnsServer）、`GET /rss.xml`（设置页只读展示地址）。
这些端点不回传 `state`。

### 静态比对结论

用脚本把 `Ops` 常量分别与 `store.mjs` 的 `case '…'` 和 `server.mjs` 的 `op === '…'` 做集合比对：

```
23 个 op 常量：全部命中真实后端（store.act 18 个 + server 5 个），无未定义、无虚构。
后端真实 op 未被客户端引用的：0 个。
代码中 Ops.X 引用：22 个，全部在常量表中定义（NotifyTest 定义但从未调用，符合决策 3）。
```

另外用 Node 起了一个**隔离实例**（`--no-toast --no-open`、临时端口、临时数据目录）实测：

```
poll                         ok=False hasState=True msg=没有启用任何数据源
no.such.op                   ok=False hasState=True msg=未知操作：no.such.op
item.add(empty)              ok=False hasState=True msg=标题不能为空
item.add(ok)                 ok=True  hasState=True msg=已加入日程   (返回 id)
source.test(bad)             ok=False hasState=True msg=找不到这个数据源
settings.save                ok=True  hasState=True
account.test(unconfigured)   ok=False hasState=True
item.remove                  ok=True  hasState=True
```

→ **「失败也必须应用返回 state」的单一数据源假设被实测证实**。`/api/state` 的
`settings` / `sources[0]` / `meta` / `meta.account` 键集合也与 `Models/` 逐项吻合。

---

## 3. 状态流与生命周期

| 阶段 | 行为 |
| --- | --- |
| 启动 | `AppInstance` 单实例 → `ProbeHealthAsync()` 先探活 → **命中已有健康服务则不显示启动态直接进正常态**；否则 spawn + 400ms 轮询、15s 超时 → `GET /api/state` → 建立单一数据源 → 启动唯一轮询 |
| 启动中 | **仅真正等待本实例 spawn 时**显示 `StartingOverlay`（中央 32px ProgressRing，无模态遮罩）+ Informational InfoBar；复用健康服务路径不显示（v1.1 §13.2 / §7.12） |
| 首次运行 | `!FirstRunCompleted && meta.lastPollAt=='' && connected` → 3 个串行 TeachingTip：①确认本地服务 ②启用数据源 ③立即抓取；可跳过；跳过后空态仍有两步入口 |
| 六状态 | 命令栏下**最多 1 条** InfoBar，按 `Error > Warning > Informational > Success` 择一：断连(Error 常驻不可关) > loginExpired(Warning) > 部分源失败/无启用源(Warning) > 启动中(Info) > 首次运行(Info) > 抓取完成/保存成功(Success 5s 自动收起)。标题下 12px 状态行常驻 |
| 抓取中 | 主按钮内文案「抓取中…」+ 禁用（`MinWidth=120` 防跳动）+ 命令栏下 2px 不确定 ProgressBar；二者表达同一状态 |
| 静默刷新 | 10s 无遮罩、无闪烁；状态行文本**只在内容实际变化时更新**，避免 LiveRegion 反复播报；写操作在途暂停合并 |
| 关闭窗口 (X / Alt+F4) | `AppWindow.Closing` → `e.Cancel=true` + `ShowWindow(SW_HIDE)`；**不发 shutdown**、托盘图标保留；首次弹一次不含启动类内容的提示 |
| 退出（托盘第 8 项） | flush 待删除项 → 停轮询 → `POST /api/shutdown`（仅 OwnsServer，3s 超时可降级放行）→ 移除托盘 / 注销 Toast → `Close()` → `Environment.Exit(0)` |
| 写操作失败 | `{ok:false, error, state}` 仍应用 `state`、回滚轻量乐观态；断连（无 state）才进 Error 流程并禁止后续写 |
| 深链 | 托盘 / 跳转列表 / Toast「打开日程」→ 激活窗口 → `NavigateTo` → 应用筛选或 `StartBringIntoView` 定位；**不压入返回栈** |

### 延迟提交与撤销（A5）· 组件 7.14「撤销条」

> **视觉规范 v1.1 §13.3 / UX 规范 §9.2 为权威修订**：通知 / 日程移除与 `notice.readAll` 回滚
> 一律走**底部居中 8s 撤销条**，不再用 `TeachingTip` 撤销、也不弹 `ContentDialog`。
> `item.add` / 手动新增属「新增型」，**不走撤销条**（Success InfoBar / 零提示）。

`MainWindow` 持有跨页存活的 `_pendingUndos`（`{Label, Undo, Commit?}`）与 1s `DispatcherTimer`：

- **容器**：`UndoBar`（`MainWindow.xaml`）内容区底部居中、最大宽 480、距底 24；
  不透明实色 + 1px 描边 + 圆角 8 + **Z=0 无投影**、**不用 Acrylic**；
  16px `Delete` 图标用 `NeutralForegroundBrush`（中性色不用红）；右侧**仅**「撤销」按钮
  (`MinWidth=64`) + 12px tabular 倒计时「8s…1s」；底部 2px **确定性** `ProgressBar` 线性递减。
- **独立状态位**：撤销条与顶部全局 InfoBar 一上一下并存，**不受「最多 1 条」择一逻辑影响**；
  `ShowSuccess()` 在撤销条存在时直接跳过，避免 8s 内叠加 Success InfoBar；不夺焦点、无模态遮罩。
- **多条目合并**：8s 内连续移除多条只保留 1 条，文案变「已移除 N 项」，撤销一次性恢复全部，不堆叠。
- **延迟提交**：`notice.remove` / `item.remove` 的 `Commit` 是 `DispatchAsync(op,{id})`，
  8s 内**不发任何写请求**；倒计时归零或 `FlushDeletesAsync()` 才提交。
- **已提交型**：`notice.readAll` 立刻生效，`Commit=null`，`Undo` 逐条 `notice.read{read:false}` 回放。
- **退出前 flush**：`FlushDeletesAsync()` 停计时器、隐藏撤销条，并在 12px 状态行写
  「正在保存 N 项变更…」（不弹窗）后提交。
- **确认策略唯一分叉**：仅 `source.remove` / `account.clear` / 清空 API Key / 清空关键词走
  `ContentDialog`（`DefaultButton=Close`）；两类路径在代码中显式区分，不混用。
- 撤销后卡片**立刻回原位**（`IsHidden=false` + 集合增量同步），无淡入动画；
  静默刷新期间因 `IsHidden` 被排除在 `Desired()` 之外，不会闪回。

> 已知缺口：撤销后「焦点落到相邻卡片」（§8.3）未实现，见第 7 节。

---

## 4. 性能设计

| 关注点 | 实现 |
| --- | --- |
| **避免不必要重渲染** | 四页各自维护 `ObservableCollection<ViewModel>`；`ApplyState` 先按 id **就地更新 VM 属性**，再用「删除多余 → 顺序对齐 Move/Insert」做增量同步。不重建整个 `ItemsSource`，保住焦点与滚动位置（针对 Web 版 `render()` 全量重建 `#view.innerHTML` 的痛点） |
| **单一刷新口径** | `StateStore` 单调版本号 + 写操作在途暂停静默刷新 + `SemaphoreSlim` 串行写队列（R6 / A4）；失败响应也应用 `state` |
| **防抖** | 4 个行为开关 400ms 合并 + 在途禁用；分组 1 表单显式一次性 `settings.save`（避免 `restartTimers()` 被反复重建） |
| **并发点击** | `poll` / `source.test` / `notice.summarize` / `llm.test` / `account.test` / `rescore` / `refill` / `settings.save` 全部在途禁用或本地 `_inFlight` 去重，直接忽略而不是排队 |
| **轮询频率** | **唯一 10s 轮询**（主理人最终裁定），同时承担 UI 静默刷新与 `meta.reminders` 增量检测，**不另起提醒轮询**。后端 `checkReminders` 每 15s 独立运行属**服务端事实**，`reminder.id` 持久且 state 保留前 20 条，不会漏提醒。写操作在途暂停合并 + 单调版本号见 UX §9.5。**规范与实现无冲突** |
| **虚拟化** | 四页用 `ItemsControl` 而非 `ListView`，**未启用 UI 虚拟化**。取舍：后端 `notices≤200` / `items≤300` 已有硬上限，且增量 VM 同步比 `ListView` 容器回收更容易保焦点。若实测量变大，可换 `ListView` + `ItemsStackPanel` 虚拟化并在 `ApplyState` 里改用 `ContainerFromItem` 定位 |
| **图片 / 字体** | 无位图资源（品牌头用 `FontIcon`，图标待补齐）；字体走系统 `Segoe UI Variable Text` + 中文系统回退，不内嵌字体 |
| **主线程** | 网络全异步；`TrayIconService` 的 Win32 消息循环在独立 STA 线程；`NodeBackendHost` 的 stdout/stderr 异步读取；无同步阻塞调用 |
| **构建体积** | 仅一个 NuGet 包 `Microsoft.WindowsAppSDK`；未引入 CommunityToolkit / MVVM 框架；`System.Text.Json` 用 BCL |

---

## 5. 三处拍板决策的代码落点

| 决策 | 唯一落点 | 复核 |
| --- | --- | --- |
| **D1 阈值 50** | `ScorePalette.ImportantThreshold = 50` / `IsImportant()`；`AppState.ImportantCount`；`NoticesPage.Desired()` 中 `"important" => ScorePalette.IsImportant(notice)`；徽标 `ScoreBadge` 由 `ScorePalette.For()` 生成四档（70/50/35）文字 + 数字 + 形状 | 全局无 `>= 55`；「重要：关注度 ≥ 50」三处明示：ToggleButton `ToolTip`/`HelpText`、激活时 `ImportantExplanation` 常驻行、本文件与 README 登记差异 |
| **D2 loginExpired Warning** | `SourceLoginExpiredBrush` 为橙色；`SourcesPage` 卡片 Warning chip + 同排「重新登录」；`MainWindow.RecomputeInfoBar()` 只出 Warning，动作 `ReloginAsync()` → `configured?account.test:跳设置页 account.save`；`RecomputeInfoBar` 的 Error 只给断连 / 整体抓取失败 | 单源 `status:'error'` 的**卡片状态条**用红，但全局 InfoBar 在部分源失败时仍是 Warning |
| **D3 单一 Toast 通道** | `server.mjs --no-toast`；`StateStore` 唯一 10s 轮询；`ToastService.ProcessReminders()` 首帧做基线（历史 id 不弹），之后按 `reminder.id` 去重且 `at > 会话起点` 才弹；`settings.notify=false` 时客户端也不弹但照样记 seen；`ShowTestNotification()` 客户端自测 | `Ops.NotifyTest` 定义但**代码中从未引用**（脚本核验 0 次调用）；设置页 `TestToast_Click` 与命令栏 `CmdTestToast_Click` 都调 `App.Toast.ShowTestNotification()` |

---

## 6. 静态自检结果（交付前实际执行）

| # | 检查 | 结果 |
| --- | --- | --- |
| 1 | `node --check server.mjs` | ✅ 通过 |
| 2 | `server.mjs` diff | ✅ **恰好 2 个 hunk**（`parseArgs` 一处 + `onNotify` 一处），不含其它改动 |
| 3 | `Ops` 常量 × 后端 `case`/`op ===` 集合比对 | ✅ 23/23 命中，0 虚构，0 未定义引用，`NotifyTest` 0 次调用 |
| 4 | Node 隔离实例实测 `/api/state` 字段形状 | ✅ `settings` / `sources[0]` / `meta` / `meta.account` 键集合与 `Models/` 逐项吻合 |
| 5 | Node 隔离实例实测 `/api/action` 成功与失败响应 | ✅ 8/8 均带 `state`；`item.add` 返回 `id` |
| 6 | 所有 `.xaml` XML 良构（`ElementTree` 解析） | ✅ 0 处错误 |
| 7 | XAML `ThemeResource` / `StaticResource` 键可解析 | ✅ 0 处未解析（局部 Page/UserControl 资源 + Themes + WinUI 系统键全覆盖） |
| 8 | `Colors.xaml` 三分支键集合一致性 | ✅ Light 92 / Dark 92 / HighContrast 92，差集均为空 |
| 9 | XAML 事件处理器 → code-behind 存在性 | ✅ 0 处缺失 |
| 10 | 代码零裸 HEX（除 `Themes/Colors.xaml`、图标说明） | ✅ 组件中 0 处 |
| 11 | 四处（设置页 / 托盘 / 跳转列表 / 首次引导）启动类关键词检索 | ✅ 命中 0 |
| 12 | 文件清单完整性 | ✅ 6 个 Services、5 个 Themes、8 个 Models(+ViewModels)、4 页 XAML+cs、3 个 Controls、Helpers、csproj / sln / 双清单 / app.manifest / Assets README / 2 份 docs |
| 13 | csproj 框架与包引用自洽 | ✅ `net8.0-windows10.0.19041.0` + `UseWinUI` + `WindowsPackageType=None` + 单一 `Microsoft.WindowsAppSDK` 包 |

> 以上均为**静态 / 后端运行时**检查，**不包含 C# 编译或 WinUI 运行时验证**。

---

## 7. 已知限制与未验证项（诚实清单）

### 7.1 环境导致的（最重要）

1. **本机未编译验证**。本机只有 .NET 8/9/10 运行时 + Node v22.22.2，**没有 .NET SDK、没有 VS2022、没有 Windows SDK 头文件**。
   整个 `winui3/` 解决方案**没有经过 `dotnet build` / MSBuild / XAML 编译器**，也没有运行过一次。
   首次构建必须在装有 VS2022 + .NET 桌面开发 + Windows 应用 SDK 工作负载的机器上进行。
2. **NuGet 版本**：`Microsoft.WindowsAppSDK` 写成 `1.6.240923002`，请按 VS 中实际安装的版本调整；
   若 VS 提示缺少 `Microsoft.Windows.SDK.BuildTools`，按实际版本显式加一个 `PackageReference`。
3. **图标资源缺失**：`Assets/` 只有 README，没有 `.ico` / `.png`。补齐步骤见 `Assets/README.md`；
   补齐前托盘图标回退系统默认图标，MSIX 清单里的图片路径为占位。

### 7.2 WinUI / Windows App SDK API 面（运行时未验证）

4. `MicaBackdrop.Kind`（`MicaKind.Base` / `BaseAlt`）通过**反射**设置，保证在不同 1.5/1.6 小版本上都能编译；
   若目标 SDK 没有该属性则退化为默认 Base（深色少一层 BaseAlt，不影响功能）。
5. `AppNotificationManager.Default.Register()` 在 **unpackaged** 应用的部分 SDK/OS 组合下可能注册 COM 激活器失败。
   `ToastService` 会 catch 并置 `IsAvailable=false`，降级为「应用内状态行 + 日程分组仍显示提醒」，不阻断启动；
   冷启动 Toast 激活用反射读取 `Arguments` 字典以兼容不同投影类型。这两条都**未经运行时验证**。
6. `Windows.UI.StartScreen.JumpList` **需要包标识**：默认 `WindowsPackageType=None`（unpackaged）时
   `LoadCurrentAsync()` 抛 `COMException`，`JumpListService` 静默降级为 no-op。
   跳转列表 3 项要生效必须改为 MSIX 打包并以打包方式启动。这是 WinRT API 固有限制，不虚构「unpackaged 也支持」。
7. `AppWindow.Closing` 拦截 X 关窗 → `ShowWindow(SW_HIDE)` 的隐藏/还原行为未在真机验证。
8. `TrayIconService` 使用 `Shell_NotifyIconW` + 专用 STA 线程 + `TrackPopupMenuEx(TPM_RETURNCMD)`，
   `NOTIFYICONDATA` 结构布局、菜单 8 项顺序与禁用态均按 Win32 规范书写，但**未运行验证**。
9. `Colors.xaml` 的 HighContrast 分支用 `<StaticResource x:Key=… ResourceKey="SystemColor…Brush"/>` 对象元素
   映射系统色；若目标 SDK 解析失败需改为代码侧按 `AccessibilitySettings` 处理。高对比度「显式关闭 Mica/Acrylic」
   已在 `MainWindow.ApplyBackdrop()` 中实现，但未运行验证。
10. `ThemeBrush.Get()` 从 `Application.Resources.MergedDictionaries` 的 ThemeDictionaries 取 Brush，
    失败回退中灰；主题切换时由 `ContentLayer.ActualThemeChanged` 触发四页重新解析色值，未运行验证。
11. Segoe Fluent Icons 码位取自视觉规范 §6.2–§6.4 的**参考码位**，`IconSparkle` / `IconPermission` /
    `IconErrorBadge` 等需在落地前按官方字形表核对一次（规范 §12 亦要求）。

### 7.3 与两份规范的已登记差异 / 简化

12. **轮询已统一为唯一 10s 一次**（主理人最终裁定）：`StateStore.PollIntervalSeconds = 10`，一条轮询同时承担 UI 静默刷新与 `meta.reminders` 增量检测，按 `reminder.id` 去重后发 Toast，**不另起 15s 提醒轮询**。后端 `checkReminders` 每 15s 独立运行是**服务端事实**，`reminder.id` 持久且 state 保留前 20 条，10s 轮询不会漏提醒。写操作在途暂停合并静默刷新 + state 单调版本号（UX §9.5）保持不变。**规范与实现无冲突**。注意与「15s 启动超时」（`NodeBackendHost.StartupTimeoutSeconds`，等待本实例 spawn 的进程就绪超时）区分，后者按 UX §7 保持 15s 不动。
13. **筛选控件**：用 3 个 `ToggleButton` 药丸态（视觉规范 §7.5 明确允许的备选），未用 `SelectorBar` / CommunityToolkit `Segmented`，
    以消除额外 NuGet 依赖与版本不确定性。
14. **More 浮层**：用系统样式 `MenuFlyout` 代替 `CommandBarFlyout`（同为 Acrylic 浮层、系统样式），
    换取 DataTemplate 内可靠的 `Tag` 路由。
15. **吸顶分组头**：实现了不透明底 + 1px 底分隔线，但**没有真正的滚动吸顶**（用嵌套 `ItemsControl` 而非 `ListView` 分组头）。
16. **`InfoBadge` 严重度样式**：只设置了 `Value` 与显示/隐藏，未套 `CautionInfoBadgeStyle`（避免依赖不确定的资源键）；
    日程临期徽标与通知未读徽标在颜色上不做区分，语义仍由状态行文字 + 徽标数字承载。
17. **`notices.refill` 的「N / 40」确定性进度条未实现**：后端是一次性返回 `message`（无逐条进度事件），
    改为按钮内 loading + 结果文案；视觉规范 §7.12 的确定性进度条在本版本无数据来源。
18. **`<640` 紧凑布局**：实现了 `LeftMinimal` + 页面最大宽 100% + 卡内边距收缩；
    设置页「双列→上下堆叠」只通过 `PageRoot` 边距/最大宽调整，未逐行切换堆叠；次要 meta 隐藏不完整。
19. **完成态 `Opacity`**：普通主题下 0.55 仅作装饰；语义由删除线 + 「已完成」文字 + `AutomationProperties.Name` 承载；
    高对比度下 `ThemeBrush.IsHighContrast()` 会把 `Opacity` 置 1.0。
20. **数据源 `map` 字段**：编辑器简化为 JSON 文本框（`Dictionary<string,string>`），
    `SourceItem.Map` 本身用 `JsonElement?` 保留任意 JSON 值，不会因反序列化失败崩。
21. **新增/编辑共用 `SourceEditor`**：编辑态是卡内可折叠 `Border`（不是 `Expander` 控件），
    视觉上是内联长表单，符合「不用 ContentDialog」的要求。
22. **每个分组右下「保存设置」**：只有分组 1 有显式保存按钮；行为开关即时保存；分组 3/4/5 各有自己的操作按钮。
23. **设置页外观偏好三选（浅/深/跟随系统）**未实现：默认不设置 `Application.RequestedTheme`，直接跟随系统（视觉规范 §9.1 默认行为）。
24. **`CmdShowDone` / `CmdAddSource` 等主命令按钮**在主窗口全局 CommandBar 上按页面切换可见性，
    而不是每页各自的 CommandBar；InfoBar 与状态行同样在全局外壳（保证「最多 1 条」）。

### 7.4 部署 / 契约假设

25. **端口固定 8737**：`ApiClient.Port` 硬编码。若用户把 `config.json` 的 `port` 改成别的值，
    自动托管的 server 会监听那个端口而客户端仍探 8737 → 15s 超时。当前契约约定 8737，未做动态发现。
26. **`server.mjs` 定位**：`ResolveBackendRoot()` 从 `AppContext.BaseDirectory` 向上最多 8 级找 `server.mjs`，
    或读 `CAMPUS_NOTICE_ROOT`。从本解决方案用 VS 启动即可满足；**MSIX 打包部署**需要把整个 Node 后端随包分发
    （或让客户端读环境变量指向已安装的后端目录），本版本未实现该部署方案。
27. **`server.mjs` 的 `notify.test` 分支**直接调 `toast()`（不经过 `onNotify`），所以 `--no-toast` 不会静默化这个 op；
    但客户端**从不调用**它，单一通道约束不受影响。
28. **任务管理器强杀**客户端进程时无法优雅 shutdown：数据因逐 op 落盘不丢，但 `RUNTIME_PATH` 可能残留；
    下次启动的 health 探测会处理。
29. 无自动化测试（本机无 SDK）；测试策略见 `docs/winui3-build-and-run.md` 的「验证清单」。

---

## 8. 与 Phase 1 / Phase 2 的交接核对

- 视觉规范 §12「给 Phase 3」：① Token 表抄进 `Themes/` ✅；② 图标码位集中定义并要求核对 ✅（核对动作留给首次构建）；
  ③ `icon.svg` 多尺寸 `.ico` 规格 + 可执行转换方案 ✅；④ 四处零启动类入口检索 ✅；
  ⑤ 中性层用系统 `ThemeResource`、HighContrast 显式关 Mica/Acrylic ✅。
- 体验规范 §10.9 逐页验收口径：op 全部映射到真实契约 ✅；延迟提交/撤销 ✅；`state` 整体刷新（失败也刷） ✅；
  六状态 InfoBar 择一 ✅；深链不压栈 ✅；完成态 Name 含「已完成」 ✅；凭据 PasswordBox ✅；零启动类入口 ✅。
- 差异与未落地项全部登记在第 7 节，**未以占位文件充数**。

---

## 9. 视觉规范 v1.1 §13 / 体验规范 v1.1 同步说明

> **重要时序说明**：Phase 3 实现过程中，`frontend-design-architect` 与 `ux-ui-designer`
> 把两份规范从 v1.0 升级到 **v1.1**（视觉规范新增 §13「v1.1 增补」为**权威修订**，
> UX 规范同步 S1–S5）。本实现已按 v1.1 重新对齐，下面逐项登记落点，供验收核对。

| v1.1 修订 | 代码落点 | 状态 |
| --- | --- | --- |
| **S1 · 组件 7.14 撤销条取代 TeachingTip 撤销**（§13.3 / §9.2 / §3.5 / §4.5 / 附录 A） | `MainWindow.xaml` 的 `UndoBar`（底部居中 480 宽、实色卡 Z=0 无投影不用 Acrylic、中性色 Delete 图标、仅「撤销」+ tabular 倒计时、2px 确定性 ProgressBar、不夺焦点、独立于顶部 InfoBar）；`MainWindow.PushUndo()` / `ScheduleDelete()` / `OnUndoTick()` / `UndoButton_Click()`；多条目合并「已移除 N 项」 | ✅ |
| **S1 · 撤销条仅服务「移除型」**；`item.add` 走 Success InfoBar、手动新增零提示、取消「查看日程」按钮（§13.3 / §3.4 / §4.2） | `NoticesPage.CommitAddScheduleAsync()` 改为 `ShowSuccess("已加入日程…")`；`SchedulePage.AddSave_Click()` 成功后零提示；`Ops.ItemRemove` 不再被新增流程直接调用 | ✅ |
| **S2 · 确认策略唯一分叉**（§13.2 / §9.3 / §10.9）：通知/日程移除 = 撤销条；数据源删除 / 清除账号 / 清空 Key = ContentDialog | `SourcesPage.Delete_Click` / `SettingsPage.AccountClear_Click` / `LlmClear_Click` 走 `Dialogs.ConfirmDestructiveAsync()`；通知/日程走 `ScheduleDelete()`；两类路径代码显式分开 | ✅ |
| **S3 · 在途忙态 5 条**（§7.8 / §6.2）：①乐观值不回弹 ②14px ProgressRing + 同组禁用 ③防抖只一个在途态 ④失败顶部 InfoBar Error ⑤成功不叠加吐司 | `SettingsPage.xaml` 4 个 `Ring*`；`SyncToggles()` 在忙态/防抖期跳过；`OnDebounceTick()` 串行提交 + `SetRingsActive()` + 失败 `App.Window.ShowError()` + `ApplyToggleValues()` 回滚；成功不弹提示 | ✅（④ 的 Error 由 `MainWindow.ShowError()` 走顶部 InfoBar） |
| **S4 · 启动时序**（§13.2 / §7.2 / §7.12）：复用健康服务时**不显示**「正在连接本地服务…」 | `NodeBackendHost.ProbeHealthAsync()`；`App.StartBackendAsync()` / `RetryConnectionAsync()` 先探活，只有 `reused == false` 才 `ShowBackendStarting()` | ✅ |
| **S5 · D1 阈值明示落点**（§13.2 / §7.5 / 附录 A）：筛选行下方 12px Caption「重要 = 关注度 ≥ 50 的通知」+「重要」项 ToolTip / AutomationProperties.Name；**不塞进药丸项**；`<640` 只保留后者 | `NoticesPage.xaml` 的 `ImportantExplanation`（`NoticeMetaBrush`）、`FilterImportant` 的 `ToolTip` / `HelpText` / `AutomationProperties.Name`；`ApplyCompact()` 在紧凑模式隐藏说明行 | ✅ |
| **S5 · Toast 通道无关 / 严禁双发**（§7.13 / §10.0 A1 / §10 R9） | 决策 3 选客户端 `AppNotification`（可带「打开日程」）；`--no-toast` 静默后端；`ToastService` 按 `reminder.id` 去重 | ✅ |
| **§13.7 输入框错误态**（表单显式保存失败 = 字段内联错误；开关失败 = 顶部 InfoBar Error） | 表单侧 `Group1Error` / `AccountError` / `LlmError` 内联 `TextBlock`（字段下方）；开关侧 `ShowError()` 走顶部 InfoBar | ⚠️ 部分：内联文案已落地，**1px `CriticalFillBrush` 描边 + 14px ErrorBadge 图标 + 逐字段 HelpText** 未实现，见第 7 节新增限制 30 |
| **附录 A 退出前 flush 提示**「正在保存 N 项变更…」（12px 状态行，不弹窗） | `MainWindow.FlushDeletesAsync()` 设置 `StatusLine.Text` | ✅ |

### 新增的已知限制（v1.1 对齐后）

30. **§13.7 输入框错误态的逐字段描边 / 图标未实现**：目前为分组级内联错误文案
    （`Group1Error` / `AccountError` / `LlmError`），没有把 `CriticalFillBrush` 1px 描边套到具体
    `TextBox` / `NumberBox` / `PasswordBox`，也没有逐字段 `AutomationProperties.HelpText`。
    反馈位置的两类区分（表单内联 vs 开关顶部 InfoBar）已实现。
31. **撤销后焦点落到相邻卡片**（§8.3 / §9.2）未实现：`UndoButton_Click()` 只恢复 `IsHidden` 与集合，
    未记录/恢复业务 id 对应的焦点元素。撤销条本身不夺焦点 ✅。
32. **日程分组头「真正滚动吸顶」**仍未实现（见第 7 节第 15 条）；v1.1 未改变该要求。
33. **轮询间隔**：已统一为 **10s**（主理人最终裁定），与 v1.1 §7.2 一致，**无冲突**；后端 15s 为服务端事实，见第 7 节第 12 条。
34. **`NoticeMetaBrush` / `NeutralFillBrush` / `NeutralForegroundBrush`** 由 `Colors.xaml` 三分支提供，
    撤销条与筛选说明行按 v1.1 指定键引用，组件零裸 HEX 仍成立。

### v1.1 之后重新执行的静态自检

```
XML 良构（含 csproj / 双 manifest）        : 0 处错误
ThemeResource/StaticResource 未解析键     : 0 处
Colors.xaml Light/Dark/HighContrast       : 92 / 92 / 92，差集为空
XAML 事件处理器 → code-behind 缺失        : 0 处
C# 花括号 / 圆括号平衡（词法扫描）        : 0 处失衡
Ops 常量 × 后端契约                       : 23/23 命中，0 虚构，NotifyTest 0 次调用
四处启动类关键词检索                       : 0 命中
```

> 仍**未编译验证**：本机无 .NET SDK / VS2022 / Windows SDK 头文件。
