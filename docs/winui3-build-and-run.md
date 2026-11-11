# WinUI 3 客户端 · 构建与运行指南

> 适用：`winui3/CampusNotice.sln`（`CampusNotice`，`net8.0-windows10.0.19041.0`）。
> 上游规范：`docs/winui3-visual-spec.md`、`docs/winui3-ux-spec.md`；架构说明：`docs/winui3-engineering.md`。

---

## 0. ⚠️ 本机未编译验证（务必先读）

产出本解决方案的机器上**只有 .NET 8/9/10 运行时 + Node v22.22.2**，
**没有 .NET SDK、没有 VS2022、没有 Windows SDK 头文件**。

因此：

- **本解决方案从未 `dotnet build` / MSBuild 编译过，也从未运行过一次**；
- 已完成的只有静态自检 + Node 后端运行时验证（详见 `docs/winui3-engineering.md` 第 6 节）；
- 下面的步骤是**给装有完整工具链的机器**用的首次构建清单，不是「已构建通过」的记录。

请在装有 **VS2022 + .NET 桌面开发 + Windows 应用 SDK** 工作负载的机器上执行，并预留处理
NuGet 版本、图标资源、HighContrast 分支与 AppNotification 注册的时间。

---

## 1. 需要的环境

| 项 | 要求 |
| --- | --- |
| OS | Windows 10 1809 (10.0.17763) 或更高；推荐 Windows 11 22H2+（Mica / `AppNotification` 体验完整） |
| IDE | Visual Studio 2022（17.4+，建议 17.8+） |
| VS 工作负载 | ① **.NET 桌面开发**；② **Windows 应用 SDK**（或「通用 Windows 平台开发」+ 单独安装 Windows App SDK） |
| SDK | Windows 11 SDK (10.0.22621.x 或更高)；.NET 8 SDK |
| Node.js | ≥ 18（本机实测 v22.22.2）。**仅运行期**需要，客户端会自动 spawn `server.mjs` |
| 权限 | 普通用户即可。`app.manifest` 为 `asInvoker`，不提权；后端只监听 `127.0.0.1` |

### NuGet 包

`CampusNotice.csproj` 里只有**一个**显式包引用：

```xml
<PackageReference Include="Microsoft.WindowsAppSDK" Version="1.6.240923002" />
```

> **按 VS 中实际安装的版本调整**。本项目用到的 API 面要求 ≥ **1.5**
> （`MicaBackdrop` 需 1.3+、`InfoBadge` 需 1.2+、`AppNotificationManager` / `SelectorBar` 级别需 1.5+），推荐 1.6。
> `Microsoft.Windows.SDK.BuildTools` 由 WindowsAppSDK **传递引用**，通常无需显式声明；
> 若 VS 报「缺少 Microsoft.Windows.SDK.BuildTools」，按本机实际版本补一条 `PackageReference`。
>
> 未引入 `CommunityToolkit.WinUI`：筛选控件改用 3 个 `ToggleButton`（视觉规范 §7.5 明确允许的备选），
> 从而少一个版本不确定的依赖。

### 关键 csproj 设置

```xml
<TargetFramework>net8.0-windows10.0.19041.0</TargetFramework>
<UseWinUI>true</UseWinUI>
<WindowsPackageType>None</WindowsPackageType>   <!-- unpackaged，免签名免打包 -->
<EnableMsixTooling>true</EnableMsixTooling>
<ApplicationManifest>app.manifest</ApplicationManifest>
<Platforms>x86;x64;ARM64</Platforms>
<RuntimeIdentifiers>win-x86;win-x64;win-arm64</RuntimeIdentifiers>
```

**不要**在补齐图标前给 csproj 加 `<ApplicationIcon>Assets\AppIcon.ico</ApplicationIcon>`——文件还不存在会直接编译失败。

---

## 2. 首次构建步骤（unpackaged，推荐）

1. 打开 `winui3/CampusNotice.sln`。
2. 右键解决方案 → **还原 NuGet 包**。若报 `Microsoft.WindowsAppSDK 1.6.240923002` 找不到，
   打开 `工具 → NuGet 包管理器 → 管理解决方案的 NuGet 程序包`，查本机可用版本后改 `CampusNotice.csproj` 的 `Version`。
3. 配置选 `Debug`，平台选 `x64`（或 `x86` / `ARM64`）。
4. `生成 → 生成解决方案`（Ctrl+Shift+B）。
5. `调试 → 开始调试`（F5）。

运行时客户端会：

```
1. GET /api/health                    → 已有健康实例？复用（不 spawn）
2. 否则 spawn `node server.mjs --no-toast --no-open`（隐藏窗口）
3. 400ms 间隔轮询 health，15s 超时
4. GET /api/state → 建立单一数据源 → 通知页 → 启动唯一 10s 静默刷新
```

**目录前提**：`NodeBackendHost.ResolveBackendRoot()` 会从 `AppContext.BaseDirectory` 逐级向上（最多 8 级）
寻找 `server.mjs`。从本解决方案用 VS 启动时，exe 在
`winui3/CampusNotice/bin/x64/Debug/net8.0-windows10.0.19041.0/`，向上能找到仓库根目录，满足前提。

若把 exe 单独拷到别处，请设置环境变量后启动：

```powershell
$env:CAMPUS_NOTICE_ROOT = "C:\Users\Administrator\Desktop\myshit\BJUT-News-Grabber"
.\CampusNotice.exe
```

### 直接运行生成的 exe

```
winui3\CampusNotice\bin\x64\Debug\net8.0-windows10.0.19041.0\CampusNotice.exe
```

同目录下的 `Microsoft.WindowsAppRuntime.Bootstrap.dll` 等运行时文件由构建自动复制，不要单独拷 exe。

---

## 3. MSIX 打包（想启用任务栏跳转列表时）

`JumpListService` 用 `Windows.UI.StartScreen.JumpList`，**该 WinRT API 需要包标识**。
默认 `WindowsPackageType=None` 时 `LoadCurrentAsync()` 会抛「无包标识」的 `COMException`，
服务已 `try/catch` 静默降级（三个入口仍可从系统托盘进入）。

让跳转列表生效：

1. 先按第 5 节补齐 `Assets/` 里 `Package.appxmanifest` 引用的所有 PNG；
2. 把 `CampusNotice.csproj` 的
   ```xml
   <WindowsPackageType>None</WindowsPackageType>
   ```
   改成
   ```xml
   <WindowsPackageType>MSIX</WindowsPackageType>
   ```
   或干脆删掉该行（VS 模板默认打包）；
3. 确认 `Package.appxmanifest` 的 `Identity / Publisher` 与本机签名证书匹配；
   `Publisher="CN=CampusNotice"` 是占位值，需要换成你自己的证书主题；
4. 右键 `CampusNotice` 项目 → **发布 → 创建应用程序包**（旁加载 / 开发用证书）；
5. 安装生成的 `.msix` 后启动。

> **注意**：MSIX 沙箱里默认不含仓库根目录的 `server.mjs` / `public/` / Node 后端。
> 打包部署需要把整个 Node 后端随包分发并用 `CAMPUS_NOTICE_ROOT` 指向可写目录，
> 或改为「客户端只复用外部已启动的 `start.cmd` 实例」。**本版本未实现该部署方案**，见工程文档 §7.4。

---

## 4. 唯一后端改动 · 默认行为不变 · 如何回滚

> **这是本次交付对现有 Node/Web 后端的唯一改动，共 2 处（2 个 diff hunk）。**

### 改动 1 — `server.mjs` `parseArgs()`（约 L28-33）

```diff
-  const args = { open: true, port: null, data: null }
+  const args = { open: true, port: null, data: null, noToast: false }
   ...
     else if (arg === '--open') args.open = true
+    /* WinUI 3 客户端托管时用 --no-toast ... */
+    else if (arg === '--no-toast') args.noToast = true
```

### 改动 2 — `server.mjs` `onNotify` 回调（约 L95-98）

```diff
   onNotify: (title, body) => {
+    /* --no-toast：后端只记日志并返回 true，不调用 toast()，避免双发 */
+    if (args.noToast) {
+      console.log('[campus-notice] 已交给客户端系统通知（--no-toast）：', title, body)
+      return true
+    }
     toast(title, body)
     return true
   },
```

### 为什么默认行为完全不变

- `noToast` 默认 `false`；不带 `--no-toast` 时 `if (args.noToast)` 分支不进入，`toast()` 照常调用。
- `start.cmd` / `ctl.mjs` / 现有 Web 版 / `node server.mjs` 都不带该参数 → **零影响**。
- `store.mjs` `recordReminder()` L627-641 是**先写 `meta.reminders` 再判断 `settings.notify`**，
  因此后端静默化**不会丢提醒记录**；客户端改为消费 `meta.reminders` 用 `AppNotification` 发系统 Toast。
- `return true` 保持不变：`store.mjs` 的 `notify.test` 分支用 `Boolean(fired)` 决定 `ok`，
  客户端托管时该 op 也不会变成 `ok:false`（不过客户端从不调用它）。

### 如何回滚（两步，任选其一）

**只撤功能、保留参数兼容**：保留改动 1，把改动 2 的 `if (args.noToast) {...}` 整段删除即可。
此后所有调用方（含客户端）都恢复后端 PowerShell Toast 通道 —— 但会与客户端 `AppNotification` **双发**，
所以回滚后必须同时把 `NodeBackendHost.Spawn()` 里的 `--no-toast` 参数去掉，
并把 `ToastService.ProcessReminders()` 改为只做去重、不 `Show()`。

**完全回到交付前状态**：

```bash
cd C:/Users/Administrator/Desktop/myshit/BJUT-News-Grabber
git checkout -- server.mjs
```

然后 `node --check server.mjs` 复核。注意：完全回滚后客户端自动托管会与后端 Toast 双发，
**不要只回滚 `server.mjs` 而保留客户端**。

### 未改动的文件（边界）

`public/`、`core.mjs`、`store.mjs`、`llm.mjs`、`login.mjs`、`notify.mjs`、根目录 `README.md`、
`start.cmd` / `stop.cmd` / `ctl.mjs` / `package.json` 全部保持原样。

---

## 5. 图标补齐（首次构建前）

`winui3/CampusNotice/Assets/` **只有 `README.md`**，没有二进制图标。规格与三种可执行转换方案
（ImageMagick / Inkscape + icotool / 手工）见 `Assets/README.md`。最小需要：

| 文件 | 用途 |
| --- | --- |
| `AppIcon.ico` | 16/20/24/32/48/256 多尺寸；补齐后可加回 `<ApplicationIcon>` |
| `TrayLight.ico` / `TrayDark.ico` | 托盘；深色任务栏用**单色白**版 |
| `StoreLogo.png` / `Square44x44Logo.png` / `Square150x150Logo.png` / `Wide310x150Logo.png` / `SplashScreen.png` | MSIX 清单引用（unpackaged 构建用不到，但补齐可避免清单校验报错） |

补齐后建议把资源标为随输出复制：

```xml
<ItemGroup>
  <Content Include="Assets\**\*.ico;Assets\**\*.png">
    <CopyToOutputDirectory>PreserveNewest</CopyToOutputDirectory>
  </Content>
</ItemGroup>
```

`TrayIconService.ResolveIcon()` 会按当前主题找 `Assets\TrayDark.ico` / `Assets\TrayLight.ico`，
找不到就回退 `LoadIcon(NULL, IDI_APPLICATION)`（不会崩，只是图标不美观）。

**图标码位核对**：`Themes/IconGlyphs.xaml` 与 `Helpers/Icons.cs` 里的 Segoe Fluent Icons 码位取自
视觉规范 §6.2–§6.4 的「参考码位」，落地前请用官方字形表核对一次；
`IconSparkle` / `IconPermission` / `IconErrorBadge` 尤其需要确认。

---

## 6. 故障排查

| 现象 | 原因 / 处理 |
| --- | --- |
| 还原 NuGet 报 `Microsoft.WindowsAppSDK 1.6.240923002` 不存在 | 按 VS 实际安装版本改 `CampusNotice.csproj` 的 `Version`；≥ 1.5 即可 |
| 报缺少 `Microsoft.Windows.SDK.BuildTools` | 显式加一条 `PackageReference`，版本按本机安装填写 |
| XAML 编译报 `XamlControlsResources` / `InfoBadge` 找不到 | WindowsAppSDK 版本过低（< 1.2）或 `UseWinUI` 未生效；升到 1.5+ |
| XAML 编译报 `Grid` 没有 `BorderBrush` / `BorderThickness` | 本交付已移除这类误用；若自行改动请改用 `Border` 包裹 |
| 报 `Package.appxmanifest` 引用的 PNG 不存在 | 按第 5 节补齐，或临时把 `<EnableMsixTooling>` 设为 `false` |
| 启动后 15s 显示「本地服务未运行」 | ① 确认 `server.mjs` 能被 `ResolveBackendRoot()` 找到（设 `CAMPUS_NOTICE_ROOT`）；② 确认 `node` 在 PATH；③ 确认 `config.json` 的 `port` 仍是 **8737**（客户端端口硬编码）；④ 点「重试」重新探测 |
| 端口被占用 / `EADDRINUSE` | 客户端启动已先 health 探测，正常不会重复 spawn；若手工起过 `start.cmd` 又启动客户端，会复用而不是再起 |
| 关掉窗口后程序不见了 | 这是设计：X = 隐藏到托盘，服务继续跑。要完全停止请在托盘菜单选「退出」 |
| 托盘图标是系统默认图标 | 按第 5 节补齐 `.ico` |
| 跳转列表 3 项不出现 | unpackaged 无包标识，属预期降级；按第 3 节改 MSIX 打包 |
| 系统 Toast 不弹 | ① `settings.notify` 是否为 false；② `ToastService.IsAvailable` 是否为 false（unpackaged 注册失败，看调试输出 `[ToastService] AppNotification 注册失败`）；③ Windows 通知设置是否关闭；④ `meta.reminders` 里的 id 是否是**本次会话启动后**新出现的（历史提醒只做基线不弹） |
| 深浅色切换后徽标颜色不对 | `ThemeBrush.Get()` 从 ThemeDictionaries 按 `App.CurrentThemeName` 取色；确认 `ContentLayer.ActualThemeChanged` 已触发；未运行时验证，见工程文档 §7.2 |
| 高对比度下 XAML 解析失败 | `Colors.xaml` 的 HighContrast 分支用 `<StaticResource … ResourceKey="SystemColor…Brush"/>` 对象元素；若目标 SDK 不支持，改为在代码侧按 `AccessibilitySettings` 处理 |

---

## 7. 首次构建后的验证清单（对应 ux-spec §10.9）

> 这些是**需要在真机上执行**的验收项，本交付**未执行**。

**启动 / 生命周期**

- [ ] 已有健康服务（Web 版或 `start.cmd`）时客户端**不 spawn** node、不出现 `EADDRINUSE`，
      且**不闪「正在连接本地服务…」启动态**（v1.1 §13.2 / §7.12）；
- [ ] 15s 启动超时后 Error InfoBar 常驻不可关 + 「重试」可恢复；等待期无模态遮罩；
- [ ] 点标题栏 X 后 `/api/health` 仍 ok、抓包**无** `/api/shutdown`、托盘图标保留；
- [ ] 托盘「退出」抓包**有** `/api/shutdown`，退出前 flush 待删除项；复用他实例服务时**不发** shutdown。

**通知页**（v1.1 §13.3：移除走底部 8s 撤销条，`item.add` 走 Success InfoBar）

- [ ] 「重要」筛选**仅**含 `score>=50`；50–54 分的橙色「关注」档必须出现；
      筛选行下方 12px Caption「重要 = 关注度 ≥ 50 的通知」+「重要」项 ToolTip / 无障碍名齐全，
      `<640` 紧凑模式只保留后者（v1.1 §13.2 / §7.5）；
- [ ] 徽标「重要 78」「关注 56」「一般 42」「低 18」「未评分」文字 + 数字 + 形状齐全，`score=0` 不是红色；
- [ ] 「移除」后 8s 内抓包无 `notice.remove`；底部撤销条（实色卡、中性色 Delete 图标、仅「撤销」+
      tabular 倒计时 + 2px 确定性 ProgressBar）与顶部 InfoBar 一上一下并存、不夺焦点；
      点「撤销」卡片**立刻回原位**且全程零写请求；多条合并为「已移除 N 项」；超时后静默刷新不使卡片闪回；
- [ ] 「全部标已读」撤销条回滚后原未读集合逐条恢复；AI 重摘要失败时摘要/score 保持原值且**不出现全局 Error InfoBar**；
- [ ] 「加入日程」成功后只出 Success InfoBar（自动 4–6s、可关闭），**无撤销条 / 无「查看日程」按钮**；
      日程页「手动添加日程」成功后**零提示**；

**日程页**

- [ ] 分组顺序恒为 已逾期→今天→明天→本周内→更远→未定时间→(已完成)；组内按 `dueAt` 升序；
- [ ] 改时间后 `fired[]` 被清空（后端语义），编辑态有 12px 说明；
- [ ] 「重置提醒」仅 `fired.length>0` 时显示；删除走 8s 撤销；
- [ ] 高对比度下完成态显示删除线 + 「已完成」文字，`AutomationProperties.Name` 含「已完成」。

**数据源页**

- [ ] `loginExpired` 卡显示 **Warning 橙** chip + 「重新登录」（非红）；`configured=true` 时点击抓包有 `account.test`；
- [ ] 全局只出 Warning InfoBar，不出 Error；
- [ ] `source.test` 成功内联 ≤5 条 preview；失败内联原因且仍应用返回 state；
- [ ] 切换 type 不清空已存隐藏字段值（保存时不传隐藏字段）；Cookie `PasswordBox` 从不回显；
- [ ] `source.remove` 走 ContentDialog，回车/Esc 默认 = 取消。

**设置页**

- [ ] 分组 1 一次点击只发 **1 次** `settings.save`；输入过程中不发请求；
- [ ] 4 个开关连点 5 次只产生 1–2 次请求；失败开关回滚；
- [ ] 凭据一律 PasswordBox、不回显，`meta.account` 无密码字段；DPAPI 说明常驻；
- [ ] 「测试桌面通知」抓包**没有** `notify.test`，只有客户端 `AppNotification` 自测 Toast；
- [ ] **设置页 / 托盘菜单 / 跳转列表 / 首次引导四处文本与资源检索，启动类关键词命中 0**。

**桌面增强 / 全局**

- [ ] 托盘 8 项顺序正确（「退出」最后），「通知 N 未读」N=0 时隐藏，「立即抓取」在 `polling` 时禁用；
- [ ] 跳转列表「常用」3 项（MSIX 打包下），无退出项、无启动类项；
- [ ] 同一 `reminder.id` 只弹 1 条 Toast，「打开日程」只定位不改状态，`notify=false` 时不弹；
- [ ] 六状态 InfoBar 同一时刻最多 1 条，按 Error>Warning>Info>Success 择一；
- [ ] Alt+1..4 切页且焦点落首控件；F5=`poll`；破坏性 ContentDialog 回车=取消；
- [ ] 10s 静默刷新不抢焦点、不重复播报状态行；
- [ ] 100 / 125 / 150% 缩放下：导航项文字不与 InfoBadge 重叠、设置页标签与输入框不挤压、通知标题两行不裁。

---

## 8. 与后端联调的小抄

```bash
# 只起服务（Web 版行为不变）
node server.mjs --no-open

# 客户端托管时实际使用的命令（隐藏窗口由客户端 CreateNoWindow 控制）
node server.mjs --no-toast --no-open

# 手工探活 / 取状态 / 发一个动作
curl http://127.0.0.1:8737/api/health
curl http://127.0.0.1:8737/api/state
curl -X POST -H "Content-Type: application/json" -d "{\"op\":\"poll\"}" http://127.0.0.1:8737/api/action

# RSS 输出
curl http://127.0.0.1:8737/rss.xml
```

> `server.mjs` 的 `sameOrigin()` 只在请求**带 `Origin` 头**时校验同源；
> 客户端 `HttpClient` 默认不带 `Origin`，所以无需额外处理。浏览器版仍带同源 Origin，行为不变。

---

## 9. 相关文件

- `winui3/README.md` — 子项目定位、依赖、目录结构、诚实性声明
- `winui3/CampusNotice/Assets/README.md` — 图标规格与转换命令
- `docs/winui3-engineering.md` — 架构、op × 页面映射、性能、已知限制与未验证项
- `docs/winui3-visual-spec.md` / `docs/winui3-ux-spec.md` — Phase 1/2 规范
