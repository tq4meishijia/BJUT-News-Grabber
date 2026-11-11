# BJUT-News-Grabber · WinUI 3 视觉设计规范（Phase 1）

> 版本 1.0 ｜ 视觉设计架构师产出 ｜ 目标框架 `net8.0-windows10.0.19041.0`（Windows App SDK / WinUI 3 / Fluent Design）
>
> **本文边界**：只定义「看起来是什么样」。不定义信息架构、交互流程、API 契约，不包含 C#/XAML 实现代码。
> **下游用途**：Phase 2（UX/交互/响应式/无障碍）据此对齐断点与状态；Phase 3（前端实现）可直接把第 10 节 Token 表抄进 `Themes/` 资源字典。
> **视觉基线来源**：现有 Web 版 `public/styles.css` / `app.js` / `icon.svg`（品牌蓝 `#2B6CFF`、铃铛+日历图标、浅/深双皮肤、12px 圆角卡片、重要度色点）。本规范是把它**翻译到原生 Fluent 体系**，而不是照搬网页。

---

## 0. 一句话结论（给赶时间的人）

**保留 Web 版的品牌蓝与数据语义色，把中性层与交互态交给 WinUI 3 系统资源；用「Mica 打底 + 描边卡片 + Acrylic 浮层」三层结构取代网页的阴影卡片；所有状态一律「颜色 + 图标 + 文字」三重编码，卡片不加投影、不覆盖系统强调色、设置页不出现任何开机自启项。**

---

## 1. 设计定位与情绪板

### 1.1 情绪关键词（5 个）

| 关键词 | 具体所指（不是形容词，是可验收的行为） |
| --- | --- |
| **安静 / Quiet** | 默认状态零弹窗、零动画、零彩色大色块；只有「红色/橙色」才被允许抢视线。抓取刷新静默进行，不加全屏遮罩、不加闪烁。 |
| **可信 / Trustworthy** | 数值（score、倒计时、时间、条数）用等宽数字右对齐；状态永远带文字说明，不让用户猜颜色；敏感字段（API Key、CAS 密码）一律掩码 + 明确「本机 DPAPI 加密」说明。 |
| **一瞥即得 / Scannable** | 每屏视觉焦点唯一：通知页是「未读 + 高重要度」，日程页是「已逾期 / 今天」，数据源页是「失败 / Cookie 失效」，设置页是「保存」；次要信息一律降到 12px Secondary。 |
| **原生 / Native** | 用系统 Mica、系统强调色、系统 Toast、系统字体 ramp、系统 InfoBar；不模拟网页、不自绘系统级 UI、不给控件套自定义皮肤。 |
| **本地 / Local** | 视觉上强调「数据在你这台机器上」：状态行常驻显示上次抓取时间/端口；断连时明确写「本地服务未运行」而不是笼统「网络错误」；不出现云同步、账号体系、在线状态等暗示。 |

### 1.2 一句话美学主张

> **「一张安静的桌面便签」——让壁纸透进来一点点，把颜色只留给真正需要你今天动手的事。**

### 1.3 为什么适合「学生每天扫一眼的本地工具」

- 使用频次高、单次停留短（10–30 秒），因此**信息密度优先于留白表演**：一屏内应能看到 4–6 条通知或 6–8 条日程，不做大 Hero 区、不做欢迎插画。
- 内容是「待办」而非「阅读」，因此**重要性靠色阶与位置表达，不靠装饰**；已读/已完成明确降权但不隐藏（学生需要回头查）。
- 本地 Node 服务可能没开、Cookie 会过期、校园网会断，因此**异常态是常态而非边缘态**：必须为「启动中 / 抓取中 / 登录失效 / 断连 / 空列表 / 首次运行」六种状态各留一个固定视觉位（第 7.9 节 InfoBar + 状态行），不让错误以弹窗打断扫视。
- 每天跟随系统浅/深色切换（早八 / 熬夜），因此**中性层必须完全交给系统资源**，自定义色只用于语义，避免深夜出现刺眼纯白或浅色下灰蒙蒙。

### 1.4 设计原则（5 条，可验收）

1. **语义色唯一来源原则**：任何红色/橙色/绿色/蓝色，必须来自第 2 节的语义 Token 或系统 Accent；组件里不出现裸 HEX。
2. **不靠颜色单独传达原则**：重要度、健康状态、临期、完成态，都必须同时具备「颜色 + 文字（或数字）+ 图标（或形状）」。高对比度模式下颜色会被系统覆盖，形状与文字是保底。
3. **描边分层、拒绝投影原则**：卡片靠 1px 描边 + 底色分层，不用 DropShadow；`ThemeShadow` 只用于 Flyout / ContentDialog / TeachingTip 三类浮层。
4. **密度恒定原则**：列表行高用 `MinHeight` + padding 表达，绝不写死 `Height`；字号全部 DIP，断点按 DIP 而非物理像素判断，保证 100/125/150% 下不裁切中文、不挤压表单。
5. **不越界原则**：不覆盖系统 `AccentColor`；不自定义托盘菜单/系统 Toast 皮肤；**设置页、托盘菜单、跳转列表、首次运行引导中，不出现「开机自启 / 随 Windows 启动 / 后台自启」任何开关、文案或引导步骤**（用户硬约束）。

---

## 2. 色彩系统

### 2.1 色彩架构（三层，决定 HEX 写在哪）

| 层 | 内容 | 实现方式 | 是否给死 HEX |
| --- | --- | --- | --- |
| A. 中性层 | 窗口底、内容区、卡片底、描边、分隔线、文字 | 引用 WinUI 3 系统 `ThemeResource` | **否**（下表 HEX 仅参考值） |
| B. 系统强调层 | 主按钮、开关 On、选中态、焦点环、导航指示条 | 引用系统 `AccentFillColorDefaultBrush` 等 | **否**（跟随用户系统主题色） |
| C. 品牌 / 语义层 | 品牌蓝、重要度 4 档、成功/警告/危险、临期、数据源健康 | 自定义 ThemeDictionary（Light / Dark / HighContrast 三套） | **是**（下表为确定值） |

> **重要取舍**：Web 版中性色是偏蓝灰的 `#F5F6F8 / #12141A`，WinUI 3 系统中性色偏中性灰。本规范**有意放弃**蓝灰中性底，改用系统资源，理由：(1) Mica 需要系统层资源才能正确着色；(2) 高对比度与系统主题切换才不会失效；(3) 品牌调性由 `icon.svg` 的蓝 + 语义色 + 品牌蓝延续，不靠背景色。若主理人坚持延续，可在 `Colors.xaml` 里用可选覆盖值，但**不推荐**。

### 2.2 品牌主色（C 层 · 确定 HEX）

| Token 键名 | 用途 | Light | Dark |
| --- | --- | --- | --- |
| `BrandBlueBrush` | Logo 底、关于页、品牌点缀、Score 中档色点 | `#2B6CFF` | `#5B8CFF` |
| `BrandBlueHoverBrush` | 品牌色块 Hover | `#4A80FF` | `#7BA3FF` |
| `BrandBluePressedBrush` | 品牌色块 Pressed | `#1B4FD0` | `#3F6FE0` |
| `BrandBlueForegroundBrush` | 浅色底上的品牌色文字/图标（≥4.5:1） | `#1B4FD0` | `#8FB0FF` |
| `BrandBlueSubtleBrush` | 品牌色淡底（徽标/InfoBar Informational） | `#E8F0FF` | `#1C2740` |

> `icon.svg` 的蓝底 `#2B6CFF` 与 Web 版 `--accent` 完全一致，浅色直接沿用；深色沿用 Web 版 `--accent: #5B8CFF`。
> **交互态不要用品牌蓝硬编码**：主按钮 / ToggleSwitch / 选中态一律用系统 `AccentFillColorDefaultBrush`。品牌蓝只用于「数据语义」与「品牌标识」，避免与用户自定义系统主题色打架。

### 2.3 语义色（C 层 · 确定 HEX）

每档拆三个变量，用途严格区分：**Fill** 只画图形/色点/状态条，**Foreground** 只写文字/图标，**Subtle** 只做徽标/条带淡底。这样能同时满足「鲜艳」和「对比度 ≥ 4.5:1」。

**Light（卡片底 ≈ `#FFFFFF`，内容层 ≈ `#F9F9F9`）**

| 语义 | Fill | Foreground（文字/图标） | Subtle（淡底） |
| --- | --- | --- | --- |
| Critical 危险 / 严重 | `#D13438` | `#B10E1C` | `#FDE7E9` |
| Warning 警告 / 临期 | `#B9761A` | `#8A5300` | `#FBF0DD` |
| Success 成功 / 正常 | `#1A9C5B` | `#0E7A42` | `#E6F6EC` |
| Info 信息 / 品牌蓝 | `#2B6CFF` | `#1B4FD0` | `#E8F0FF` |
| Neutral 中性 / 低优先 | `#8A94A6` | `#667085` | `#F0F2F5` |

**Dark（卡片底 ≈ `#272727`，内容层 ≈ `#202020`）**

| 语义 | Fill | Foreground（文字/图标） | Subtle（淡底） |
| --- | --- | --- | --- |
| Critical | `#FF6B6B` | `#FF99A4` | `#3B2024` |
| Warning | `#F5B544` | `#F7C565` | `#3A2C12` |
| Success | `#3DDC97` | `#6CE5B1` | `#12301F` |
| Info | `#5B8CFF` | `#8FB0FF` | `#1C2740` |
| Neutral | `#98A2B3` | `#98A2B3` | `#232830` |

**对比度校验（Foreground 对卡片底）**：Light 全部 ≥ 4.7:1（`#667085` 约 4.7、`#B10E1C` 约 6.3、`#8A5300` 约 5.9、`#0E7A42` 约 5.4、`#1B4FD0` 约 5.9）；Dark 全部 ≥ 6.5:1。满足 WCAG AA 正文标准。**Fill 色不用于小字号文字**（如 `#D13438` 在白底约 4.0:1，只画色点/竖条/大图标）。

### 2.4 重要度分级色阶（score 0–100）

沿用 Web 版 `scoreTone()` 的 70 / 50 / 35 三档边界，扩为 4 档并命名：

| Token 键名 | 区间 | 档位文案 | Light Fill / FG | Dark Fill / FG | 形状（高对比度保底） |
| --- | --- | --- | --- | --- | --- |
| `ScoreCriticalBrush` | 70–100 | 重要 | `#D13438` / `#B10E1C` | `#FF6B6B` / `#FF99A4` | 实心圆 + 左竖条 |
| `ScoreHighBrush` | 50–69 | 关注 | `#B9761A` / `#8A5300` | `#F5B544` / `#F7C565` | 实心菱形 |
| `ScoreMediumBrush` | 35–49 | 一般 | `#2B6CFF` / `#1B4FD0` | `#5B8CFF` / `#8FB0FF` | 空心圆 |
| `ScoreLowBrush` | 0–34 | 低 | `#8A94A6` / `#667085` | `#98A2B3` / `#98A2B3` | 小方点 |
| `ScoreNoneBrush` | 缺失 / 0 未评分 | 未评分 | 同 Neutral | 同 Neutral | 无 |

> **⚠️ 需主理人/后端确认的一处不一致**：Web 版色点分级阈值为 `70 / 50 / 35`，但「重要」筛选阈值是 `score >= 55`（`app.js` 第 231 行）。视觉上会出现「明明是橙色关注档(50–54)，却被算进重要筛选」的错位。**视觉建议：把筛选阈值对齐到 50**，使「重要」= `ScoreHigh + ScoreCritical`，视觉与逻辑自洽。若后端阈值不可改，则在筛选按钮文案改为「关注及以上」并在徽标上明确写出 50 分线。本规范默认按 **50** 设计徽标与筛选样式。

配套键名：`ScoreBadgeBackgroundBrush`（取对应 Subtle）、`ScoreBadgeForegroundBrush`（取对应 FG）、`ScoreDotBrush`（取对应 Fill）、`ScoreAccentStripBrush`（卡片左竖条，取对应 Fill）。

### 2.5 截止时间 / 临期色阶（日程与通知共用）

| Token 键名 | 触发条件 | 取色 | 说明 |
| --- | --- | --- | --- |
| `DeadlineOverdueBrush` | 已逾期 | Critical | 分组头、倒计时 chip、日程行左条 |
| `DeadlineTodayBrush` | 今天到期，或剩余 ≤ 2 小时 | Critical | 与逾期同色，但文字必须写「今天 HH:mm」以区分 |
| `DeadlineSoonBrush` | 明天 / 剩余 ≤ 24 小时 | Warning | 顶部状态行的「N 项 24 小时内到期」也用此色 |
| `DeadlineWeekBrush` | 本周内 | Info（品牌蓝） | 不用 Success，避免「已完成」语义混淆 |
| `DeadlineFarBrush` | 更远 | Neutral | — |
| `DeadlineNoneBrush` | 未定时间 | Neutral | 虚线 chip 或 `—` 占位 |

> 提醒档位（默认提前 1 天 + 2 小时）本身**不用警示色**，作为中性 chip 呈现；只有「该提醒已触发/即将触发」才用 `DeadlineSoonBrush`。

### 2.6 数据源健康与全局状态色

| Token 键名 | 状态 | 取色 | 附加视觉 |
| --- | --- | --- | --- |
| `SourceStatusOkBrush` | `status: ok` | Success | 4px 状态条 + `Completed` 图标 + 「正常 · N 条」 |
| `SourceStatusErrorBrush` | `status: error` | Critical | 4px 状态条 + `ErrorBadge` 图标 + 失败原因文字 |
| `SourceStatusIdleBrush` | `status: idle` | Neutral | 灰状态条 + `Clock` 图标 + 「尚未抓取」 |
| `SourceLoginExpiredBrush` | `loginExpired: true` | Warning（**不是** Critical） | 「Cookie 已失效」chip + 「重新登录」操作按钮 |
| `SourceCookieOkBrush` | `hasCookie: true` 且未失效 | Success | 「已存 Cookie」chip |
| `SourceDisabledBrush` | `enabled: false` | Neutral | 整卡 `Opacity = 0.6`，状态条去饱和，开关 Off |
| `BackendStartingBrush` | 后端启动中 | Info | `ProgressRing` 32px + 「正在连接本地服务…」 |
| `PollingBrush` | 抓取进行中 | Accent（系统） | 按钮内 `ProgressRing` 16px + 命令栏下 2px `ProgressBar` |
| `NetworkErrorBrush` | 连不上 `127.0.0.1` / 源错误 | Critical | 常驻 `InfoBar` Severity=Error + 「重试」 |
| `FirstRunBrush` | 首次运行 | Info | `TeachingTip` 或 Informational `InfoBar` |

> **为什么 `loginExpired` 用 Warning 而不是沿用 Web 版的 danger**：它是「需要你去点一下重新登录」的可操作待办，不是致命故障；用橙色与「抓取失败 / 断连」的红色分级，能减少学生对红色警报的钝化。红色只留给真正阻断使用的状态。

### 2.7 中性层 / 描边 / 文字（A 层 · 引用系统资源，HEX 仅参考）

| 语义用途 | 建议引用的 WinUI 3 系统键 | Light 参考 | Dark 参考 |
| --- | --- | --- | --- |
| 内容区底层（覆盖 Mica 保证可读） | `LayerFillColorDefaultBrush` | `#F9F9F9` @ ~80% | `#202328` @ ~80% |
| 内容区实体底（列表密集/吸顶分组头） | `SolidBackgroundFillColorBaseBrush` | `#F3F3F3` | `#202020` |
| 卡片底（默认 / 悬停） | `CardBackgroundFillColorDefaultBrush` / `CardBackgroundFillColorSecondaryBrush` | `#FFFFFF` / `#FAFAFA` | `#272727` / `#2E2E2E` |
| 卡片描边（默认 / 悬停） | `CardStrokeColorDefaultBrush` / `ControlStrokeColorDefaultBrush` | `#E3E6EA` | `#3A3F4B` |
| 二级面板底（正文展开区、chip） | `SubtleFillColorSecondaryBrush` / `ControlFillColorSecondaryBrush` | `#F0F2F5` | `#232830` |
| 分隔线 | `DividerStrokeColorDefaultBrush` | `#E3E6EA` | `#2B3140` |
| 主文字 | `TextFillColorPrimaryBrush` | `#191C20` | `#E9ECF1` |
| 次要文字（meta / hint） | `TextFillColorSecondaryBrush` | `#667085` | `#98A2B3` |
| 三级/禁用文字 | `TextFillColorTertiaryBrush` / `TextFillColorDisabledBrush` | `#98A2B3` | `#6E7686` |
| 输入框底 / 描边 | `ControlFillColorDefaultBrush` / `ControlStrokeColorDefaultBrush` | `#FFFFFF` / `#E3E6EA` | `#2B2B2B` / `#3A3F4B` |
| 主按钮 / 开关 On / 焦点 | `AccentFillColorDefaultBrush` + `AccentFillColorSecondaryBrush`(Hover) + `AccentFillColorTertiaryBrush`(Pressed) | 系统 | 系统 |
| 焦点可见框 | `FocusStrokeColorOuterBrush` / `FocusStrokeColorInnerBrush` | 系统 | 系统 |
| 遮罩 | `SmokeFillColorDefaultBrush` | 系统 | 系统 |
| 高对比度全组 | `SystemColorWindowBrush` / `SystemColorWindowTextBrush` / `SystemColorHighlightBrush` | 系统 | 系统 |

### 2.8 XAML ResourceDictionary 键名建议（语义别名层）

建议 Phase 3 建立一层**语义别名**，把业务键指向 A/B 层系统资源或 C 层自定义色，组件只引用业务键。命名规则：`<对象><状态/属性>Brush`，禁止以颜色名命名（如 `RedBrush` 一律禁止）。

- 通知：`NoticeCriticalBrush` `NoticeHighBrush` `NoticeMediumBrush` `NoticeLowBrush` `NoticeUnreadStripBrush` `NoticeReadTitleBrush` `NoticeCardBrush` `NoticeCardHoverBrush` `NoticeMetaBrush` `NoticeBadgeBackgroundBrush` `NoticeBodyPanelBrush`
- 重要度：`ScoreCriticalBrush` `ScoreHighBrush` `ScoreMediumBrush` `ScoreLowBrush` `ScoreNoneBrush` `ScoreDotBrush` `ScoreBadgeForegroundBrush`（完整表见 2.4）
- 日程：`GroupOverdueBrush` `GroupTodayBrush` `GroupTomorrowBrush` `GroupWeekBrush` `GroupFarBrush` `GroupDoneBrush` `DeadlineOverdueBrush`…`DeadlineNoneBrush`（见 2.5）`RemindChipBrush` `ItemDoneBrush`
- 数据源：`SourceStatusOkBrush` `SourceStatusErrorBrush` `SourceStatusIdleBrush` `SourceLoginExpiredBrush` `SourceCookieOkBrush` `SourceDisabledBrush`（见 2.6）
- 全局：`BackendStartingBrush` `PollingBrush` `NetworkErrorBrush` `FirstRunBrush` `InfoBarInfoBrush` `InfoBarSuccessBrush` `InfoBarWarningBrush` `InfoBarErrorBrush`
- 品牌：`BrandBlueBrush` `BrandBlueHoverBrush` `BrandBluePressedBrush` `BrandBlueForegroundBrush` `BrandBlueSubtleBrush`

> 所有别名必须同时出现在 `Light` / `Dark` / `HighContrast` 三个 `ThemeDictionaries` 分支中；`HighContrast` 分支里 C 层色全部改映射到系统 `SystemColor*`（见第 9 节）。

---

## 3. 材质与层次

### 3.1 三层材质结构（严格分工，不叠加）

```
┌─ 浮层 Layer 3 ── Acrylic（AcrylicInApp / AcrylicTransient）
│   Flyout · MenuFlyout · CommandBar 溢出 · TeachingTip
│   ContentDialog 本体 · 托盘菜单（系统提供，不自绘）
│   → ThemeShadow Z = 32（Flyout/TeachingTip）/ 64（ContentDialog）
├─ 卡片 Layer 2 ── 不透明实色 + 1px 描边，Z = 0，无投影
│   CardBackgroundFillColorDefaultBrush + CardStrokeColorDefaultBrush
├─ 内容 Layer 1 ── 半透明 LayerFillColorDefaultBrush（覆盖 Mica 保证文字可读）
│   列表密集页 / 吸顶分组头可用 SolidBackgroundFillColorBaseBrush 全不透明
└─ 窗口 Layer 0 ── Mica（Window.SystemBackdrop）
    NavigationView 窗格透明 → Mica 直接透出；内容区不透明 → 隔开壁纸
```

### 3.2 Mica

| 项 | 规定 |
| --- | --- |
| 应用位置 | `MainWindow.SystemBackdrop = MicaBackdrop`，仅窗口根层，全应用唯一一处 |
| Light | `MicaKind.Base` |
| Dark | `MicaKind.BaseAlt`（更暗，避免深色下壁纸过亮） |
| 高对比度 | **关闭 Mica**，回退 `SolidBackgroundFillColorBaseBrush`（必须显式在 HighContrast 分支处理，不能只依赖系统自动） |
| NavigationView 窗格 | 背景透明，让 Mica 透出；`OpenPaneLength = 240`，`CompactPaneLength = 48` |
| 内容区 | 必须铺 Layer 1 背景，**不允许**文字直接压在 Mica 上 |
| 避免 | 卡片再套 Mica；Mica 与 Acrylic 同层叠加；给 Mica 加自定义 tint 覆盖成品牌蓝；内容区写死 `#FFFFFF` / `#000000` |

### 3.3 Acrylic

| 项 | 规定 |
| --- | --- |
| 用在哪 | 仅**瞬态浮层**：`Flyout` / `MenuFlyout` / `CommandBarOverflowPresenter` / `TeachingTip` / `ContentDialog` 本体 / 托盘菜单（由系统菜单提供，不自定义） |
| 类型 | 应用内浮层 `AcrylicInApp`；可轻量消失的提示 `AcrylicTransient` |
| 不用于 | 卡片、列表行、导航窗格、页面背景、设置卡、InfoBar（这些用不透明实色） |
| 圆角 | 浮层统一 `CornerRadius = 8` |
| 描边 | 1px `ControlStrokeColorDefaultBrush`，保证浅色下浮层边界可见 |
| 避免 | 在 Acrylic 浮层里再放半透明卡片；用 Acrylic 做常驻面板（会持续耗电且与 Mica 打架） |

### 3.4 卡片 Elevation / 描边 / 圆角（具体数值）

| 属性 | 值 | 说明 |
| --- | --- | --- |
| 卡片 Elevation | **Z = 0，无 DropShadow** | WinUI 3 Fluent 卡片靠「底色 + 1px 描边」分层。Web 版 `--shadow: 0 6px 20px …` 必须去掉 |
| 卡片圆角 | `CornerRadius = 8` | 从 Web 版 12 收敛到 8，与 WinUI 3 控件圆角一致 |
| 卡片描边 | 1px `CardStrokeColorDefaultBrush`；Hover 换 `ControlStrokeColorDefaultBrush` | 深浅色都要可见 |
| 卡片内边距 | 16px（通知卡 / 设置卡）；12px（日程行 / 数据源行） | 见第 5 节 |
| 卡片间距 | 8px | 列表内；分组之间 16px |
| 状态竖条 | 3px（未读 / 重要度，贴卡片左边缘）；4px（数据源健康条，贴卡片顶边或左边） | 不用整卡淡底，避免彩色大色块 |
| 浮层圆角 / Z | 8 / `ThemeShadow` Z=32（Flyout、TeachingTip）；Z=64（ContentDialog） | 通过 `Translation="0,0,32"` 配合共享 `ThemeShadow` |
| 其他圆角 | Button / TextBox / ComboBox / NumberBox / PasswordBox = 4；Chip / Badge = 4；导航选中指示条 = 2；InfoBar = 8 | 全应用只有 2 / 4 / 8 三档 |
| 焦点框 | 2px，系统 `FocusStrokeColor*`，外扩 2px | 键盘导航必须可见 |
| 避免 | 阴影卡片、渐变边框、发光效果、彩色整卡背景、超过 8 的圆角、多层嵌套圆角容器 | |

---

## 4. 字体与字号阶梯

### 4.1 字体栈（中文环境）

| 优先级 | 字体 | 用途 |
| --- | --- | --- |
| 1 | `Segoe UI Variable Text` | Win11 默认可变字体，拉丁与数字 |
| 2 | `Segoe UI` | 旧版 / 回退 |
| 3 | `Microsoft YaHei UI`（系统中文回退） | 简体中文正文，WinUI 3 会自动回退，无需显式指定 `Noto Sans SC` |
| 4 | `Microsoft YaHei` | 再回退 |
| 图标字体 | `Segoe Fluent Icons` | 仅用于 `FontIcon`，不混排进正文 |

- `FontFamily` 写法建议：主 `Segoe UI Variable Text`，中文交给系统字体回退；如需显式，用逗号分隔的 family 列表并注明回退顺序。
- **数字**：score、倒计时、时间、条数、端口一律启用等宽数字（tabular figures，`Typography.NumeralAlignment = Tabular` / 字体特性）；若 SDK 版本不提供该附加属性，退化为「固定宽度容器 + 右对齐 + `CharacterSpacing = 0`」，避免列表里数字左右跳动。
- **避免**：中文用斜体（WinUI 会做假斜体，极难看）；中文用 `Light(300)` 字重压在 Mica 上；正文小于 12 DIP；英文标签全部大写（中文界面无意义且降低扫读）；一套界面混用 3 种以上字重；用 emoji 代替图标。

### 4.2 字号阶梯（对齐 WinUI 3 内置 Type Ramp）

| Token 键名 | 字号 (DIP) | 字重 | 行高 (px) | WinUI 锚点 | 用途 |
| --- | --- | --- | --- | --- | --- |
| `FontPageTitle` | 28 | SemiBold 600 | 36 | `TitleTextBlockStyle` | 每页标题（通知 / 日程 / 数据源 / 设置） |
| `FontDialogTitle` | 20 | SemiBold 600 | 28 | `SubtitleTextBlockStyle` | ContentDialog / Expander 标题 |
| `FontSectionTitle` | 14 | SemiBold 600 | 20 | `BodyStrongTextBlockStyle` | 设置分组标题、日程分组头（配合字距） |
| `FontCardTitle` | 14 | SemiBold 600 | 20 | `BodyStrongTextBlockStyle` | 通知标题、日程标题、数据源名（未读/未完成） |
| `FontCardTitleRead` | 14 | Regular 400 | 20 | `BodyTextBlockStyle` | 已读通知标题、已完成日程标题 |
| `FontBody` | 14 | Regular 400 | 20 | `BodyTextBlockStyle` | AI 一句话摘要、正文、对话框正文 |
| `FontBodyStrong` | 14 | SemiBold 600 | 20 | `BodyStrongTextBlockStyle` | 需强调的正文行 |
| `FontCaption` | 12 | Regular 400 | 16 | `CaptionTextBlockStyle` | meta、hint、chip、状态行、备注、URL |
| `FontOverline` | 11 | Medium 500 | 14 | Caption 变体 | 重要度徽标文字、分组头字母间距、Badge |
| `FontNumeric` | 14 | SemiBold 600 + tabular | 20 | 自定义 | score、倒计时、条数、时间 |
| `FontNumericLarge` | 20 | SemiBold 600 + tabular | 28 | 自定义 | 空态/概览大数字（可选） |

- 中文行高一律 **1.4–1.5 倍**（14→20、12→16、11→14、28→36），用显式 `LineHeight`，不要依赖默认值——150% 缩放下默认行高容易裁切中文下半部。
- 字距：`FontSectionTitle` / `FontOverline` 可加 `CharacterSpacing ≈ 20`（约 0.2px @100%），延续 Web 版 `.section-title` 的 0.4px letter-spacing 感；正文不加字距。
- 标题层级**最多 3 级**（PageTitle → SectionTitle → CardTitle），不要再加 16/18px 中间档。

---

## 5. 间距与密度

### 5.1 4/8px 基准网格

| Token | 值 (DIP) | 典型用途 |
| --- | --- | --- |
| `Space1` | 4 | 图标与文字、chip 内间距、色点间隙 |
| `Space2` | 8 | 卡片间距、按钮间距、chip 之间 |
| `Space3` | 12 | 紧凑卡片内边距、分组内行距 |
| `Space4` | 16 | 标准卡片内边距、分组头上间距 |
| `Space5` | 20 | 设置卡水平内边距、段落间距 |
| `Space6` | 24 | 页面左右边距、分组之间、表单标签与控件最小间距 |
| `Space8` | 32 | 页面下边距、大区块分隔 |
| `Space10` | 40 | 空态上下留白 |
| `Space12` | 48 | 命令栏高度、设置行高、首次运行留白 |

### 5.2 页面与内容区

| 项 | 标准（≥640 DIP） | 紧凑（<640 DIP） |
| --- | --- | --- |
| 内容区左右边距 | 24 | 12 |
| 内容区上 / 下边距 | 24 / 32 | 16 / 24 |
| 内容最大宽度（通知 / 日程 / 数据源） | 940（居中，超宽窗口不拉伸卡片） | 100% |
| 内容最大宽度（设置） | 720（居中） | 100% |
| 命令栏高度 | 48 | 48（次要项进溢出菜单） |
| 命令栏下状态区高度 | 状态行 20 + InfoBar 按需 | 同 |

> 940px 直接沿用 Web 版 `.view { max-width: 940px }`。**超宽窗口不把卡片拉满**：过长的行会显著损害扫读，必须在最大宽度两侧留 Mica 空白。

### 5.3 行高（一律 `MinHeight`，不写死 `Height`）

| 元素 | MinHeight | 内边距 |
| --- | --- | --- |
| 通知卡片 | 96（含标题 + meta + 1–2 行摘要） | 16 |
| 日程行 | 56 | 12 / 10 |
| 数据源卡片 | 88 | 12 |
| 设置开关行 | 48 | 16 / 20 |
| 设置输入行 | 44 | 16 / 20 |
| 筛选 Segmented 项 | 32 | 12 / 8 |
| 日程分组头 | 28（吸顶时不透明底 + 1px 底分隔线） | 上下 16 / 8 |
| 导航项 | 40 | 12 / 8 |
| 按钮 / 输入框 | 32 | 12 / 8 |
| chip / 徽标 | 20–21 | 8 / 4 |

### 5.4 紧凑模式阈值（按 DIP，不按物理像素）

| 有效宽度（DIP） | NavigationView | 内容布局 | 视觉降级 |
| --- | --- | --- | --- |
| ≥ 1008 | `Left` 展开，Pane 240 | 单列列表（最大宽 940），设置页双列标签/控件 | 完整 meta、完整命令栏 |
| 640 – 1007 | `LeftCompact`，Pane 48 | 单列，最大宽自适应 | 命令栏次要项进 `...`，meta 保留 |
| < 640 | `LeftMinimal`（汉堡） | 单列 100% 宽，卡内边距 12 | 隐藏次要 meta（联系人、第 2 行标签）、命令栏仅保留「立即抓取」+ 溢出、设置行由左右两列改为上下堆叠 |

> 断点判断依据 `XamlRoot.Size` / `ActualWidth`（DIP），**不要**用物理像素；150% 缩放下物理宽度变小属于正常，逻辑宽度才是判断依据。

---

## 6. 图标语言

### 6.1 使用原则

1. **优先 `SymbolIcon` + `Symbol` 枚举**（稳定、无需码位、随 SDK 走）；枚举没有的（如 AI 摘要、登录失效）才用 `FontIcon` + `Segoe Fluent Icons` 字形。
2. 所有 FontIcon 码位**集中定义在 `Themes/IconGlyphs.xaml` 的 `x:String` 常量**里（如 `IconSparkle`），组件只引用常量；落地前用 Segoe Fluent Icons 官方字形表核对一次，避免 SDK 版本差异。下表中「参考码位」仅供定位，**不作为最终依据**。
3. 尺寸：导航项 / 命令栏 **16px**；列表内辅助图标 **14px**；空态大图标 **32–40px**（`Opacity 0.5` + Neutral 色）；状态徽标内图标 **12–14px**。
4. 颜色：默认 `TextFillColorSecondaryBrush`；激活/选中 `AccentFillColorDefaultBrush`；语义状态取对应语义 **Foreground** 色（保证对比度）。
5. **图标绝不单独承载语义**：必须配文字，或 `ToolTip` + `AutomationProperties.Name`。纯图标按钮在紧凑模式下尤其危险。
6. **避免**：混用多套图标集；给图标加投影/渐变/彩色填充（Fluent 图标是单色）；用 emoji；用图标表达「开机自启」这类不存在的功能；同一含义在不同页用不同字形。

### 6.2 四页导航图标

| 页面 | 首选字形 | 参考码位 | 备选 | 理由 |
| --- | --- | --- | --- | --- |
| 通知 | `Ringer` | `\uEA8F` | `Mail`（`\uE715`，Symbol.Mail） | 呼应 `icon.svg` 铃铛，且与系统 Toast / 托盘语义一致 |
| 日程 | `Calendar` | `\uE787` | — | 与 `icon.svg` 日历元素一致 |
| 数据源 | `Globe` | `\uE774` | `List` / `Library` | 对应 4 个校内门户网址；比「数据库」更贴近「网页源」 |
| 设置 | `Setting` | `\uE713` | — | 放 NavigationView 底部（`IsSettingsVisible` / PaneFooter） |

### 6.3 状态图标

| 状态 | 字形 / 参考码位 | 颜色 | 附加 |
| --- | --- | --- | --- |
| 未读 | 实心圆点 8px（非图标）+ 加粗标题 | `BrandBlueBrush` / 对应 Score 色 | 延续 Web 版：未读靠「圆点 + 字重」，不额外加图标 |
| 已读 | 无点 + Regular 字重 | `TextFillColorSecondaryBrush` | 不加「已读」勾图标，避免噪音 |
| 正常 / ok | `Completed`（`\uE73E`, Symbol.Accept） | Success FG | 圆形徽标 + 「正常」文字 |
| 失败 / error | `ErrorBadge`（`\uEA39`，需核对） | Critical FG | + 失败原因文字 |
| 尚未抓取 / idle | `Clock`（`\uE823`，需核对） | Neutral FG | + 「尚未抓取」 |
| Cookie 已失效 | `Warning`（`\uE7BA`）+ `Permission`/管理员字形 | Warning FG | + 「重新登录」按钮 |
| 抓取中 / 启动中 | `ProgressRing`（非字形） | Accent / Info | 见 7.12 |
| 空列表 | `Mail` 或 `Inbox` 32–40px，`Opacity 0.5` | Neutral | + 两行引导文字（启用数据源 → 立即抓取） |
| 首次运行 | `Info`（`\uE946`） | Info | TeachingTip / Informational InfoBar |

### 6.4 操作图标

| 操作 | 字形 / 参考码位 | Symbol 枚举可用 |
| --- | --- | --- |
| 立即抓取 | `Refresh` `\uE72C` | `Symbol.Refresh` ✔ |
| 添加（通知源 / 日程） | `Add` `\uE710` | `Symbol.Add` ✔ |
| 筛选 | `Filter` `\uE71C` | `Symbol.Filter` ✔ |
| 全部标已读 | `Completed` / `MailRead`（需核对） | `Symbol.Accept` ✔（简化） |
| AI 摘要 / 重摘要 | `Sparkle`（**需核对码位**，无枚举） | ✘ → FontIcon 常量 `IconSparkle` |
| 加入日程 | `Calendar` + `Add` 小角标（或直接文字按钮） | `Symbol.Calendar` ✔ |
| 展开 / 收起正文 | `ChevronDown` `\uE70D` / `ChevronUp` | 需核对 |
| 编辑 | `Edit` `\uE70F` | `Symbol.Edit` ✔ |
| 删除 / 移除 | `Delete` `\uE74D` | `Symbol.Delete` ✔ |
| 测试 | `Play` `\uE768` | `Symbol.Play` ✔ |
| 保存 | `Save` `\uE74E`（需核对） | 需核对 |
| 打开原文 | `Link` `\uE71B` + `↗` 语义 | `Symbol.Link` ✔ |
| 溢出 / 更多 | `More` `\uE712` | `Symbol.More` ✔ |
| 下载 / 补齐正文 | `Download` `\uE896`（需核对） | 需核对 |
| 关键词 / 标签 | `Tag` `\uE8EC` | 需核对 |
| 联系人 | `Contact` `\uE77B`（需核对） | `Symbol.ContactInfo` ✔（近似） |
| 提醒 / 时间 | `Clock` `\uE823` | 需核对 |
| API Key / CAS 账号 | `Permission` / `Admin`（需核对） | `Symbol.Admin` ✔（近似） |
| 关闭 / 取消 | `Cancel` `\uE711` | `Symbol.Cancel` ✔ |

---

## 7. 组件视觉规范

> 通用规则：所有控件用 WinUI 3 原生样式（`AccentButtonStyle`、`DefaultButtonStyle` 等），**不重绘模板**；只在需要时用 `BasedOn` 覆盖颜色/圆角/间距。所有交互控件最小命中区 32×32 DIP。

### 7.1 NavigationView

| 项 | 规定 |
| --- | --- |
| 模式 | `PaneDisplayMode = Auto`；≥1008 `Left`（`OpenPaneLength = 240`）、640–1007 `LeftCompact`（48）、<640 `LeftMinimal` |
| 返回按钮 | `IsBackButtonVisible = Collapsed`（4 个平级顶级页，无栈式导航；页面内返回用面包屑/内容区按钮，不用系统返回键） |
| 窗格背景 | 透明，Mica 透出；不加 Acrylic、不加实色 |
| PaneHeader | 品牌区：`icon.svg` 转 24×24 圆角位图资源（需备 @1.25x/@1.5x 或矢量 `BitmapIcon`），标题「校园通知 · 日程」15px SemiBold，副标题状态行 12px `TextFillColorSecondaryBrush`（延续 Web 版 `.brand`） |
| 导航项 | 高 40，圆角 4，图标 16 + 文字 14；选中项背景 `SubtleFillColorSecondaryBrush`，左侧 3px×20px 圆角 2 的指示条 `AccentFillColorDefaultBrush` |
| 计数徽标 | `InfoBadge` 右对齐：通知未读数 = Informational（Accent）；日程 24h 内到期数 = Caution（Warning）；其余不显示；>99 显示 `99+`；0 隐藏 |
| 底部 | 设置项固定在 PaneFooter；其上可放 12px 状态摘要「服务运行中 · 端口 xxxx」。**不放任何自启/托盘开关** |
| 避免 | 自定义彩色窗格、给导航项加阴影、用 Tab 下划线样式做顶级导航、把 4 个页塞进 Top 模式（中文长标题在 Top 模式易挤） |

### 7.2 顶部命令栏 + 页面标题区

| 项 | 规定 |
| --- | --- |
| 结构 | 每页内容区顶部 Grid：左「页面标题 28px `FontPageTitle`」+ 下一行「状态行 12px Caption」；右 `CommandBar` 高 48 |
| 状态行内容 | 延续 Web 版 `#status-line`：「上次抓取 MM-DD HH:mm · 每 N 分钟自动抓一次 · N 项 24 小时内到期」；数字用 `FontNumeric`，「N 项 24 小时内到期」用 `DeadlineSoonBrush` |
| 主按钮 | 「立即抓取」`AccentButtonStyle` + `Refresh` 16px，`MinWidth = 120`（防止 loading 换文案时宽度跳动） |
| 次要按钮 | 「开启桌面通知」「测试桌面通知」等用 `DefaultButtonStyle`；`DefaultLabelPosition = Right`（宽屏）/ `Collapsed`（紧凑） |
| 溢出 | `CommandBarOverflowPresenter` 用 Acrylic，圆角 8，1px 描边 |
| polling 态 | 图标位置换 `ProgressRing` 16px、文案「抓取中…」、按钮 `IsEnabled = False`；命令栏下方再出 2px 不确定 `ProgressBar`（`PollingBrush`），二者只表达同一状态，不重复堆叠多个 ring |
| 避免 | 命令栏放超过 3 个主按钮、把破坏性操作（删除）放主按钮位、状态行用主文字色抢焦点 |

### 7.3 通知卡片

| 项 | 规定 |
| --- | --- |
| 容器 | `CardBackgroundFillColorDefaultBrush` + 1px `CardStrokeColorDefaultBrush`，`CornerRadius = 8`，内边距 16，间距 8，**无阴影** |
| 未读态 | 卡片左 3px 竖条（`Score*Brush` 对应 Fill）+ 标题左侧 8px 实心圆点 + 标题 `FontCardTitle`（600）；**不**给整卡降透明度、**不**加彩色淡底 |
| 已读态 | 无竖条、无圆点，标题 `FontCardTitleRead`（400），meta 与操作区保持可读（Web 版仅改字重，延续） |
| 行 1 | 重要度徽标（20px 高，见 7.4）+ 标题（可点击打开原文，Hover 加下划线）+ 右侧「未读 / AI 摘要 / 截止」chip 组 |
| 行 2 | meta：`来源名 · 发布日期`，12px `TextFillColorSecondaryBrush`；后接 chip：截止（`DeadlineSoonBrush` Subtle 底 + FG 字）、联系人、标签（最多 4 个，超出的收进 `+N`），chip 高 21 圆角 4 |
| 行 3 | AI 一句话摘要 `FontBody`，最多 2 行 `TextTrimming`；无摘要时整行不占位 |
| 行 4 | 「对我有什么用」清单：最多 3 条，12px Caption，每条前 4px 小方点（Neutral），可折叠；超出用「展开更多」 |
| 行 5 | 操作区右对齐：主操作「加入日程」`AccentButtonStyle`（小尺寸 28–32 高）；其余「展开正文 / 标已读 / AI 重摘要 / 移除」收进 `CommandBarFlyout`（`More` 图标）。**移除**在菜单内用 `NetworkErrorBrush` 前景 + `Delete` 图标；**不弹 ContentDialog**，改为「即时乐观移除 + 8s 撤销条」（见 §13.3 新增 7.14），因为后端无恢复已删除对象的 op，真撤销只能在请求发出前发生 |
| 展开正文 | `NoticeBodyPanelBrush`（二级面板底）圆角 6、内边距 12、`MaxHeight = 340` 可滚动、`TextWrapping = Wrap`、12.5px→ 用 `FontCaption` 16 行高 |
| Hover / Focus | 背景 → `CardBackgroundFillColorSecondaryBrush`；描边 → `ControlStrokeColorDefaultBrush`；键盘焦点 2px 系统焦点框 |
| 避免 | 每张卡铺彩色大色块、给卡片加投影、把 5 个操作按钮全铺开、用颜色深浅表示已读（应靠字重+圆点）、标题固定高度截断中文 |

### 7.4 重要度徽标（Score Badge）

| 项 | 规定 |
| --- | --- |
| 尺寸 | 高 20，圆角 4，水平内边距 8 |
| 内容格式 | **「{档位} {score}」**，例如「重要 78」「关注 56」「一般 42」「低 18」「未评分」。`FontOverline` 11px Medium + `FontNumeric` 数字 |
| 配色 | 底 = 对应档位 Subtle；文字/图标 = 对应档位 Foreground；左侧 8px 圆点或 3px 竖条 = 对应档位 Fill |
| 形状冗余 | 4 档同时用不同形状（实心圆 / 实心菱形 / 空心圆 / 小方点），高对比度下靠形状+文字区分 |
| 位置 | 卡片标题左侧第一行；同时驱动卡片左 3px 竖条颜色 |
| **硬性要求** | **必须带文字与数字，绝不只用颜色**；`AutomationProperties.Name` 写完整「重要度 78 分，重要」 |
| 避免 | 只画一个色点、用连续渐变色阶（离散 4 档才可读）、score=0 显示成红色、徽标用 Fill 色做小字（对比度不足） |

### 7.5 筛选 Segmented / ComboBox

| 项 | 规定 |
| --- | --- |
| 首选控件 | `SelectorBar`（WASDK 1.5+）或 CommunityToolkit `Segmented`；备选 3 个 `ToggleButton` / `RadioButtons`，**不自绘** |
| 项 | 「全部 N」「重要 N」「未读 N」，数字用 `FontNumeric` 右对齐；「重要」按 **score ≥ 50**（见 2.4 说明） |
| 尺寸 | 高 32，圆角 6（外容器）/ 4（单项），项内边距 12/8 |
| 选中态 | 底 `AccentFillColorSecondaryBrush`（或 `SubtleFillColorSecondaryBrush` + 2px 底部指示条 `AccentFillColorDefaultBrush`），文字 `TextFillColorPrimaryBrush` SemiBold |
| 同排右端 | 「全部标已读」`DefaultButtonStyle`，无数据时禁用 |
| 层级区分 | **不要用 Tab 下划线样式**——顶级导航已经在左侧，页内筛选若再用下划线会混淆信息层级；用填充块/药丸态区分 |
| 避免 | 筛选与导航长得一样、选中态只靠细下划线、把筛选塞进 ComboBox 却不带计数 |

### 7.6 日程分组头 + 日程行

**分组头**

| 项 | 规定 |
| --- | --- |
| 顺序 | 已逾期 → 今天 → 明天 → 本周内 → 更远 → 未定时间 →（可选）已完成 |
| 文字 | `FontSectionTitle` 14px SemiBold 或 `FontOverline` 11px Medium + 字距；格式「{组名} · {N}」，N 用 `FontNumeric` |
| 色 | 已逾期 / 今天 = `GroupOverdueBrush`（Critical FG）；明天 = Warning FG；本周内 = Info FG；更远 / 未定 = Neutral FG；已完成 = Success FG + `Opacity 0.7` |
| 装饰 | 组名左 4px 高 14 的色条（取对应 Fill），上下间距 16 / 8 |
| 吸顶 | 滚动吸顶；吸顶时背景必须**不透明**（`SolidBackgroundFillColorBaseBrush`），底部 1px `DividerStrokeColorDefaultBrush`，不能让下方文字透上来 |

**日程行**

| 项 | 规定 |
| --- | --- |
| 容器 | 卡底 + 1px 描边，圆角 8，`MinHeight = 56`，内边距 12/10，行间距 8 |
| 左 | 16px `CheckBox`（圆角 2，Accent 填充），命中区扩到 32×32 |
| 标题 | `FontCardTitle`；完成态加删除线 + `FontCardTitleRead`；整行 `Opacity = 0.55`（**不低于 0.5**） |
| 右侧 chip 组 | 时间（12px tabular）｜倒计时（tabular，「还有 2 小时 10 分」）｜提醒档位（`Clock` 14px + 「提前 1 天 · 2 小时」）｜「来自通知」Info chip |
| 临期 | 逾期 → 时间与倒计时 chip 用 `DeadlineOverdueBrush` Subtle 底 + FG 字；≤24h → `DeadlineSoonBrush`；本周内 → `DeadlineWeekBrush` |
| 备注 | 12px Caption Secondary，最多 1 行省略；「查看通知原文 ↗」用 Accent FG + `Link` 图标 |
| 操作 | 「改时间 / 重置提醒 / 删除」收进 `More` 溢出菜单，行内不铺开 |
| 避免 | 完成态把整行降到几乎看不见、给每个 chip 都用不同颜色、倒计时用比例字体导致跳动、分组头吸顶时半透明 |

### 7.7 数据源状态条 + 数据源卡片

| 项 | 规定 |
| --- | --- |
| 状态条 | 卡片**顶边或左边 4px** 色条：`SourceStatusOkBrush`(绿) / `ErrorBrush`(红) / `IdleBrush`(灰)；停用态去饱和为 Neutral |
| 首行 | `ToggleSwitch`（启用/停用，On = 系统 Accent）｜名称 `FontCardTitle`｜类型 chip（html/rss/json，Neutral）｜状态 chip（「正常 · 12 条」「失败 · 超时」「尚未抓取」：Subtle 底 + FG 字 + 状态图标） |
| 登录态 | `loginExpired` → Warning chip「Cookie 已失效」+ `Warning` 图标 + 同排「重新登录」文字按钮；`hasCookie` → Success chip「已存 Cookie」；无 Cookie → 不显示或 Neutral「无需登录」。**三者互斥，不同时出现** |
| 次行 | URL 12px Caption Secondary 单行省略 + `Link` 16px；备注 12px |
| 右侧 | 「测试 / 编辑 / 删除」；删除用 `NetworkErrorBrush` 前景 + `Delete` 图标 + ContentDialog 确认 |
| 尾部元信息 | 右对齐一组 tabular 数字「上次 HH:mm · N 条」12px Caption |
| 停用态 | 整卡 `Opacity = 0.6`、状态条 Neutral、开关 Off；文字仍保持可读 |
| 状态冗余 | 每条状态必须「色条 + 图标 + 文字」三者齐备，缺一不可 |
| 避免 | 只用绿/红圆点表示健康、把 loginExpired 与 error 混成同一种红、Cookie 明文或长串直接显示、启用开关放在卡片最右导致扫视断点不一致 |

### 7.8 设置卡片

| 项 | 规定 |
| --- | --- |
| 宽度 | 720 居中；分组：抓取与关键词 / 行为开关 / 自动登录（CAS）/ AI 摘要 / 运行状态 |
| 分组标题 | `FontSectionTitle` 14px SemiBold，色 `TextFillColorSecondaryBrush`，上间距 24、下间距 8；下一组前 `Space6` |
| 卡片 | 圆角 8 + 1px 描边；内边距 16/20；行 `MinHeight = 48`；行间 1px `DividerStrokeColorDefaultBrush` |
| 开关行 | 左：标题 14px + 描述 12px Secondary（两行，解释副作用，如「关掉后走规则摘要」）；右：`ToggleSwitch` 右对齐，与文字最小间距 24 |
| **在途忙态**（A4） | 后端 `settings.save` 每次都会 `restartTimers()`，4 个行为开关的即时保存需串行 + 300–500ms 防抖 + 在途禁写。视觉：① 切换后**保持乐观值不回弹**；② 该开关 `IsEnabled = False`，其右侧紧邻 14px `ProgressRing`，同组其他开关一并短暂禁用；③ 防抖窗口内不重复出现 ring / 吐司；④ 失败回滚到旧值 + 顶部 InfoBar Error（**不弹窗**）；⑤ 成功后**不**弹 Success 吐司，避免与 §7.9 Success 条叠加。表单类（pollMinutes / keywords / autoRemindBefore）仍走显式「保存设置」一次性提交，不参与此队列 |
| 输入行 | 左标签 14px；右控件宽 240–280：`NumberBox`（抓取间隔，min 5）、`TextBox`（关键词、CAS 入口/回调地址、学号）、`PasswordBox`（API Key、CAS 密码）、`ComboBox`（提醒档位预设） |
| 敏感字段 | API Key / CAS 密码**一律 `PasswordBox` + 显示切换**；已保存时 placeholder「已保存（留空不修改）」，**绝不回显明文**；配 `Admin`/`Permission` 图标 + 12px 说明「本机 DPAPI 加密，仅当前 Windows 用户可解」 |
| 保存 | 每组右下 `AccentButtonStyle`「保存设置」，高 32 |
| 运行状态区 | 只读信息 12px Caption + tabular（上次抓取、累计次数、数据文件路径、端口、RSS 地址）；`lastPollError` 用 InfoBar Error；次要按钮「测试桌面通知」「按当前关键词重算摘要」「补齐通知正文」 |
| **硬约束** | **整页不出现「开机自启 / 随 Windows 启动 / 后台自启 / 最小化启动」任何开关、文案、图标或说明**；托盘相关只描述「关闭窗口后可在托盘找到」，不描述启动行为 |
| 避免 | 把 10+ 字段塞进一个 ContentDialog、明文显示凭据、开关行没有描述文字、把保存按钮放在每组左上、用彩色卡片区分设置分组 |

### 7.9 InfoBar（全局状态唯一载体）

| Severity | 触发 | 取色 | 行为 |
| --- | --- | --- | --- |
| Informational | 后端启动中、首次运行引导、一般提示 | `InfoBarInfoBrush` = Info | 可关闭，操作按钮「打开设置」 |
| Success | 抓取完成、保存成功、测试通过 | Success | 自动 4–6s 收起，可关闭 |
| Warning | 登录态失效、Cookie 需更新、部分源失败 | Warning | `IsClosable = True`，带「重新登录 / 查看数据源」操作 |
| Error | 连不上 `127.0.0.1`、网络/源错误、抓取失败 | Critical | **常驻**（`IsClosable = False`）直到状态恢复，带「重试」 |

| 项 | 规定 |
| --- | --- |
| 位置 | 命令栏正下方，全宽但不超过内容最大宽；同一时刻**最多 1 条**（按 Error > Warning > Info > Success 优先级择一） |
| 撤销反馈（独立于本体系） | **8s 撤销条独立于全局状态位体系（非例外、非第 5 种状态）**，可用 `InfoBar`（`Severity=Success`、`IsClosable=True`、`ActionButton=撤销`）或独立底部实色条；**不受本节「同一时刻最多 1 条」择一逻辑影响**，因此与顶部常驻 Error/Warning 一上一下并存、**不得叠在同一位置**。纯成功吐司仍沿用上表 Success 态；撤销条完整规格见 §13.3 |
| 视觉 | 圆角 8，左侧 4px Severity 色条（原生 InfoBar 已带），Title 14px SemiBold，Message 12px Caption，图标 16px，ActionButton 用 `DefaultButtonStyle` |
| 避免 | 用 InfoBar 做瞬时吐司（用 TeachingTip / 系统 Toast）、同时叠 3 条、把可恢复错误做成可关闭后不再提示、用 Error Severity 表达「登录失效」（用 Warning） |

### 7.10 ContentDialog

| 项 | 规定 |
| --- | --- |
| 用途 | **仅**用于：不可逆 / 高影响的破坏性确认（删除数据源、清除已保存账号）、短表单（≤4 字段：手动添加日程、通知改时间）、首次运行引导 |
| 不用 ContentDialog | **删除通知 / 删除日程**改为「即时移除 + 8s 撤销条」（见 §13.3）：后端无恢复已删除对象的 op，撤销只能在请求发出前生效；弹确认框会让用户被打断两次且撤销失去意义。数据源删除 / 清除账号因不可逆且后端无撤销能力，**仍保留 ContentDialog**。这是全应用唯一的确认策略分叉，实现时必须显式区分 |
| 尺寸 | 宽 480–560，圆角 8，内边距 24，`MaxHeight ≤ 窗口高 80%` |
| 材质 | 遮罩 `SmokeFillColorDefaultBrush`；本体 Acrylic 或 `SolidBackgroundFillColorBaseBrush` + 1px `ControlStrokeColorDefaultBrush`；`ThemeShadow` Z=64 |
| 文字 | Title `FontDialogTitle` 20px SemiBold；正文 `FontBody` 14；次要说明 12px Caption |
| 按钮 | `DefaultButton = Close`（破坏性操作**绝不**默认回车确认）；确认按钮用 Critical FG 文字「删除」+ `Delete` 图标；`CloseButtonText = 取消` |
| 长表单 | 数据源编辑（10+ 字段）**不用 ContentDialog**，改用内联可折叠 `Expander` 卡片（延续 Web 版行为），避免超高对话框 |
| 首次运行 | 3 步引导（确认本地服务 → 启用数据源 → 立即抓取），建议用分步 `TeachingTip` 串联而非大对话框 |
| 避免 | 对话框里再嵌对话框、把成功提示做成模态、确认按钮默认高亮为破坏性操作、对话框中出现任何「开机启动」授权 |

### 7.11 TeachingTip

| 项 | 规定 |
| --- | --- |
| 用途 | 首次运行功能指引、指向具体控件的说明（如指向「立即抓取」、指向 DataSource 的 Cookie 字段） |
| 视觉 | Acrylic (`AcrylicTransient`)，圆角 8，1px 描边，宽 320–360，Z=32；Title 14px SemiBold + 12px 说明 + 「知道了」`DefaultButtonStyle` |
| 图标 | `HeroContent` 仅放 32px 单色 Fluent 图标（Info / Ringer），**不放照片或插画** |
| 行为 | `IsLightDismissEnabled = True`；`PreferredPlacement = Bottom / Right` 按目标控件位置选；一次只显示 1 个，串行引导 |
| 避免 | 指向不可见/被折叠的控件、一次弹多个、用 TeachingTip 报错（用 InfoBar）、引导里含任何自启授权步骤 |

### 7.12 进度指示

| 状态 | 控件 | 规格 |
| --- | --- | --- |
| 后端启动中 | 内容区中央 `ProgressRing` 32px + 14px「正在连接本地服务…」+ 12px 提示 | **不用**模态遮罩阻塞窗口 |
| 抓取进行中 | 主按钮内 `ProgressRing` 16px（换文案，按钮 `MinWidth` 固定）+ 命令栏下 2px 不确定 `ProgressBar` | 二者表达同一状态，不重复多个 ring |
| AI 摘要 / 测试中 | 对应按钮内 `ProgressRing` 16px + 「摘要中… / 测试中…」，按钮禁用 | 保持按钮宽度不变 |
| 已知进度（补齐正文 N/40） | `ProgressBar` 确定性，放在 InfoBar 内或命令栏下 2px | 显示「12 / 40」tabular 数字 |
| 静默刷新 | 状态行文字 + 徽标数字更新，**不加**全屏遮罩、不加闪烁动画 | 延续 Web 版 10s 静默轮询 |
| 避免 | 同屏多个 ProgressRing、用动画 GIF/Lottie、进度条与按钮 loading 各说一套、加载时让列表空白闪烁 |

### 7.13 托盘菜单 / 跳转列表 / 原生 Toast（桌面增强）

| 项 | 规定 |
| --- | --- |
| 托盘图标 | `icon.svg` 转多尺寸 `.ico`（16/20/24/32/48，备 @125%/@150%）；另备单色白版用于深色任务栏；有未读/临期时叠加小圆点 Badge（Critical Fill）或数字，不用整图标变色 |
| 托盘菜单 | 用系统菜单样式（Acrylic 由系统提供），**不自绘彩色/圆角皮肤**；项顺序：灰显标题（应用名 + 「服务运行中」/「服务未连接」）→「显示主窗口」→「立即抓取」→「通知 N 未读」「日程 N 项今日到期」→ 分隔线 →「退出」；图标 16px 单色，勾选态用系统复选 |
| 任务栏跳转列表 | 分组「常用」= 通知 / 日程 / 立即抓取；图标复用四页图标；不塞彩色自定义图标；**不含任何启动项** |
| 原生 Toast（通道无关） | 无论由客户端 `AppNotification` 还是后端 `onNotify` 发出，**视觉一律交给系统**：应用图标 + 标题「日程提醒 · {title}」+ 正文（≤2 行）+ 时间；强调色跟随系统，**不硬编码品牌蓝**；不自绘气泡。必须按 `reminder.id` 去重，同一 id 只允许 1 条，**严禁双发** |
| 「打开日程」激活按钮 | **仅客户端 `AppNotification` 通道可用**（点击 → 激活窗口 → 导航日程页）。若采用「后端托管 Toast」，此按钮不可用，退化为「点击 Toast 本体激活窗口并跳转日程页」，跳转由客户端激活事件承担。通道二选一（A1，待 team-lead 拍板），本文不预设实现架构 |
| **硬约束** | 托盘菜单、跳转列表、首次运行引导中**不出现「开机自启 / 随 Windows 启动」任何入口** |
| 避免 | 自绘气泡通知替代系统 Toast、Toast 里塞长正文（≤2 行）、托盘菜单做渐变/圆角自定义皮肤、把「退出」放在菜单顶部 |

---

## 8. 四个页面的视觉线框描述

> 通用外壳（四页共享，不重复描述）：窗口 Mica 底 → 左侧透明 NavigationView 窗格（品牌头 + 4 项 + 计数 InfoBadge + 底部设置）→ 内容区不透明 Layer 底 → 顶部标题 + 状态行 + 命令栏 → 状态带（InfoBar，最多 1 条）→ 主体列表（最大宽 940/720 居中）。**视觉焦点每页唯一**，其余信息降到 12px Secondary。

### 8.1 通知页

- **分区**：① 标题「通知」+ 状态行；② 命令栏（立即抓取 / 开启桌面通知 / 溢出）；③ 筛选行（Segmented 三段 + 右端「全部标已读」）；④ 通知卡片流；⑤ 空/错误态。
- **主次**：焦点是**未读且高重要度**的卡片——它们同时具备「左 3px 红/橙竖条 + 8px 实心圆点 + SemiBold 标题 + 重要度徽标」，在灰底列表里形成一条自上而下的彩色竖线，眼睛沿竖线扫即可。已读卡片只剩细描边与常规字重，退到背景。
- **焦点保护**：一屏内彩色竖条不宜超过 3 条；若高分未读很多，靠筛选而非加深颜色解决。meta / 摘要 / 标签全部 12–14px，AI 一句话摘要是最先被读的正文，故紧跟标题；「对我有什么用」清单折叠，默认只露 1–2 条。
- **空/错**：空列表用 32px `Mail` 低透明图标 + 两行引导（先去数据源启用 → 再点立即抓取）；断连/失效由顶部 InfoBar 承担，卡片区不做整块遮罩。

### 8.2 日程页

- **分区**：① 标题「日程」+ 状态行（含「N 项 24 小时内到期」橙色数字）；② 命令栏（＋手动添加日程 / 显示已完成开关 / 右端「共 N 项待办」）；③ 内联添加表单（展开时才出现，折叠态不占位）；④ 分组列表；⑤ 空态。
- **主次**：焦点是**「已逾期」与「今天」两组**——分组头的 Critical 色条 + 颜色文字 + 吸顶不透明底，构成时间轴上的红色起点；往下依次橙色（明天）、蓝色（本周内）、灰色（更远），形成一条自上而下「由紧到松」的色温梯度。组内行以 CheckBox 为左锚点，右侧 chip 组右对齐，倒计时用等宽数字形成整齐竖列。
- **焦点保护**：完成态整行降到 `Opacity 0.55` 且不加任何彩条，避免与临期项争夺注意力；提醒档位 chip 保持中性，不参与色温梯度。
- **空/错**：无日程时居中图标 +「点＋手动添加，或在通知里一键加入日程」。

### 8.3 数据源页

- **分区**：① 标题「数据源」+ 状态行；② 命令栏（＋添加数据源 / 溢出）；③ 内联添加表单（展开时）；④ 4 张数据源卡片（对应北工大预置 4 个平台）；⑤ 底部 12px 说明区（校内网/WebVPN/DNS 覆盖/JSON 解析等）。
- **主次**：焦点是**失败与 Cookie 失效**的卡片——顶部 4px 红/橙状态条是横向扫描线；正常卡片只有绿细条与 12px「正常 · N 条」，视觉上安静。所有卡片左对齐同一 ToggleSwitch 位置，使「启用/停用」形成一列整齐开关轴，便于一眼确认哪几个源在抓。
- **焦点保护**：URL、备注、类型 chip 一律 12px Secondary；「已存 Cookie」用 Success 但只做小 chip，不做整卡绿底；底部说明区用最弱层级，且单行不超过 80 字符宽。
- **空/错**：无源时引导「添加第一个数据源」；单个源失败不影响其他卡片，错误只在该卡与顶部 InfoBar 表达。

### 8.4 设置页

- **分区**：① 标题「设置」+ 状态行；② 五个分组（抓取与关键词 / 行为开关 / 自动登录 CAS / AI 摘要 / 运行状态），每组一个 720 宽卡片，组间 24px；③ 无侧栏、无命令栏主按钮（保存按钮在各组右下）。
- **主次**：焦点是**各组右下的「保存设置」按钮**（系统 Accent 填充），形成一条右侧对齐的纵轴；每行左侧「标题 + 描述」是扫读单元，右侧控件是操作单元，中间用 1px 分隔线切行。敏感字段（API Key、CAS 密码）用 PasswordBox + 锁形图标 + 12px 加密说明，形成明确的「谨慎区」视觉信号。
- **焦点保护**：运行状态区全部 12px 只读信息 + tabular 数字，不用彩色卡片；行为开关 4 行保持同一节奏，不因为重要就放大；**整页没有任何开机自启开关**。
- **空/错**：`lastPollError` 用 InfoBar Error 内嵌在状态组；凭据保存成功用 Success InfoBar 自动收起，不打断表单填写。

---

## 9. 深浅色切换 / 高对比度 / 缩放适配

### 9.1 深浅色切换

| 项 | 规定 |
| --- | --- |
| 默认 | `RequestedTheme` 跟随系统（延续 Web 版 `color-scheme: light dark`）；设置页可选提供「跟随系统 / 浅色 / 深色」三选（属外观偏好，非信息架构决策） |
| 实现 | 所有自定义色放 `ThemeDictionaries` 的 `Light` / `Dark`（+ `HighContrast`）分支；组件只引用语义别名，代码里**零裸 HEX** |
| 材质 | Light → `MicaKind.Base`；Dark → `MicaKind.BaseAlt`；切换时同步换 backdrop，不重启、不闪烁 |
| 校验 | 2.3 节两套语义 FG 对各自卡片底均 ≥ 4.5:1；`BrandBlueForegroundBrush` 在浅色用 `#1B4FD0`（不用 `#2B6CFF`，后者做小字对比度不足） |
| 图标 | 托盘 `.ico` 备浅/深两套；单色图标随 `TextFillColorSecondaryBrush` 自动反相 |
| 避免 | 深浅色各写一份组件模板、深色直接把浅色值调暗 20%（语义色必须重新选，见 2.3）、深色下用纯黑 `#000000` 卡片、浅色下 Mica 上直接放白字 |

### 9.2 高对比度模式

| 项 | 规定 |
| --- | --- |
| 材质 | **显式关闭 Mica / Acrylic**，全部回退 `SolidBackgroundFillColorBaseBrush`；不要依赖系统自动 |
| 颜色 | C 层所有语义别名在 `HighContrast` 分支映射到 `SystemColorWindowTextBrush` / `SystemColorHighlightBrush` / `SystemColorWindowBrush`；禁用自义 HEX |
| **状态冗余（保底）** | 重要度 4 档靠**形状 + 文字**（实心圆 / 实心菱形 / 空心圆 / 小方点 + 「重要 78」）；数据源健康靠**图标 + 文字**（Completed / ErrorBadge / Clock + 「正常 · N 条」）；临期靠**文字**（「今天 HH:mm」「已逾期」）；未读靠**圆点 + 字重 + 「未读」文字**。颜色此时可能被系统完全覆盖，形状与文字是唯一保底 |
| 边界 | 卡片与背景必须有可见 1px 边界（`SystemColorWindowTextBrush`）；禁用 `Opacity` 表达状态（高对比度下会失效），改用文字/删除线/形状；已完成态用删除线 + 「已完成」文字，不用降透明度 |
| 焦点 | 2px `SystemColorHighlightBrush`，与背景对比 ≥ 3:1；键盘焦点绝不只靠颜色 |
| 动效 | 尊重系统「减少动画」，关闭 TeachingTip/列表插入过渡 |
| 无障碍命名 | 所有徽标、图标按钮、chip 必须设 `AutomationProperties.Name`（如「重要度 78 分，重要」「数据源 教务处，状态失败，Cookie 已失效」） |
| 避免 | 高对比度下仍用彩色竖条当唯一线索、把信息藏进 ToolTip、用 `Opacity 0.55` 表示完成态 |

### 9.3 缩放 100 / 125 / 150%

| 项 | 规定 |
| --- | --- |
| 单位 | 字号/间距/圆角全部 DIP，WinUI 3 自动按 DPI 缩放；**禁止**用物理像素判断布局 |
| 布局 | 所有行高用 `MinHeight` + padding，不写死 `Height`；不固定 `TextBlock.Height`；`TextWrapping = Wrap`，单行 meta 才用 `TextTrimming = CharacterEllipsis` |
| 行高 | 显式 `LineHeight`（见 4.2），防止 150% 下中文下半部被裁 |
| 断点 | 按 `XamlRoot.Size` / `ActualWidth` 的 **DIP** 判断（≥1008 / 640–1007 / <640），150% 下窗口逻辑宽度变小会自然触发紧凑模式，这是预期行为 |
| 表单 | 设置行用 `Grid` 两列 + `MinWidth`，<640 DIP 时切上下堆叠；标签与输入框最小间距 24，125% 下不得互相挤压 |
| 数字 | 等宽数字 + 右对齐；`InfoBadge` 与导航项文字之间留 8px，125% 下不得重叠 |
| 命中区 | 所有交互控件 ≥ 32×32 DIP（缩放后自动放大），CheckBox 视觉 16px 但命中区扩到 32 |
| 图标资源 | `.ico` / 位图备 @1.25x / @1.5x，避免 150% 下铃铛图标模糊 |
| 最小字号 | 11 DIP（仅徽标/Overline），正文 ≥ 12 DIP；不因缩放再降字号 |
| 验收清单 | ① 125%：导航项文字不与 InfoBadge 重叠；② 150%：设置页标签与输入框不换行挤压；③ 150%：通知标题两行不被裁；④ 三档下状态行 12px 均不溢出命令栏；⑤ 三档下吸顶分组头不遮挡行内容 |

---

## 10. 设计 Token 汇总表（Phase 3 直接抄进 `Themes/`）

> 建议文件组织：`Themes/Colors.xaml`（ThemeDictionaries：Light / Dark / HighContrast）、`Themes/Brushes.xaml`（语义别名指向 Colors 与系统 ThemeResource）、`Themes/Typography.xaml`（字号/行高/字重/字体栈）、`Themes/Spacing.xaml`（Thickness / CornerRadius / Double）、`Themes/IconGlyphs.xaml`（`x:String` 图标常量）。组件只引用 `Brushes.xaml` 的别名，不直接引用 `Colors.xaml`。

### 10.1 颜色 Token（Light / Dark 确定值；HighContrast 列填系统映射规则）

| Token 键名 | 语义用途 | Light | Dark | HighContrast |
| --- | --- | --- | --- | --- |
| `BrandBlueBrush` | 品牌蓝 / 中档重要度 Fill | `#2B6CFF` | `#5B8CFF` | `SystemColorHighlightBrush` |
| `BrandBlueHoverBrush` | 品牌块 Hover | `#4A80FF` | `#7BA3FF` | 同上 |
| `BrandBluePressedBrush` | 品牌块 Pressed | `#1B4FD0` | `#3F6FE0` | 同上 |
| `BrandBlueForegroundBrush` | 品牌色文字/图标 | `#1B4FD0` | `#8FB0FF` | `SystemColorHighlightTextBrush` 或 `SystemColorHighlightBrush` |
| `BrandBlueSubtleBrush` | 品牌淡底 | `#E8F0FF` | `#1C2740` | `SystemColorWindowBrush` |
| `CriticalFillBrush` | 危险图形/色点/条 | `#D13438` | `#FF6B6B` | `SystemColorWindowTextBrush` |
| `CriticalForegroundBrush` | 危险文字/图标 | `#B10E1C` | `#FF99A4` | `SystemColorWindowTextBrush` |
| `CriticalSubtleBrush` | 危险淡底 | `#FDE7E9` | `#3B2024` | `SystemColorWindowBrush` |
| `WarningFillBrush` | 警告图形/条 | `#B9761A` | `#F5B544` | `SystemColorWindowTextBrush` |
| `WarningForegroundBrush` | 警告文字/图标 | `#8A5300` | `#F7C565` | `SystemColorWindowTextBrush` |
| `WarningSubtleBrush` | 警告淡底 | `#FBF0DD` | `#3A2C12` | `SystemColorWindowBrush` |
| `SuccessFillBrush` | 成功图形/条 | `#1A9C5B` | `#3DDC97` | `SystemColorWindowTextBrush` |
| `SuccessForegroundBrush` | 成功文字/图标 | `#0E7A42` | `#6CE5B1` | `SystemColorWindowTextBrush` |
| `SuccessSubtleBrush` | 成功淡底 | `#E6F6EC` | `#12301F` | `SystemColorWindowBrush` |
| `InfoFillBrush` | 信息图形 | `#2B6CFF` | `#5B8CFF` | `SystemColorHighlightBrush` |
| `InfoForegroundBrush` | 信息文字 | `#1B4FD0` | `#8FB0FF` | `SystemColorHighlightBrush` |
| `InfoSubtleBrush` | 信息淡底 | `#E8F0FF` | `#1C2740` | `SystemColorWindowBrush` |
| `NeutralFillBrush` | 中性图形/低分色点 | `#8A94A6` | `#98A2B3` | `SystemColorWindowTextBrush` |
| `NeutralForegroundBrush` | 中性文字 | `#667085` | `#98A2B3` | `SystemColorWindowTextBrush` |
| `NeutralSubtleBrush` | 中性淡底 / chip | `#F0F2F5` | `#232830` | `SystemColorWindowBrush` |
| `ScoreCriticalBrush` | 70–100 重要 | `#D13438` | `#FF6B6B` | `SystemColorWindowTextBrush` |
| `ScoreHighBrush` | 50–69 关注 | `#B9761A` | `#F5B544` | `SystemColorWindowTextBrush` |
| `ScoreMediumBrush` | 35–49 一般 | `#2B6CFF` | `#5B8CFF` | `SystemColorHighlightBrush` |
| `ScoreLowBrush` | 0–34 低 | `#8A94A6` | `#98A2B3` | `SystemColorWindowTextBrush` |
| `ScoreNoneBrush` | 未评分 | `#8A94A6` | `#98A2B3` | `SystemColorWindowTextBrush` |
| `DeadlineOverdueBrush` | 已逾期 | `#D13438` | `#FF6B6B` | `SystemColorWindowTextBrush` |
| `DeadlineTodayBrush` | 今天 / ≤2h | `#D13438` | `#FF6B6B` | `SystemColorWindowTextBrush` |
| `DeadlineSoonBrush` | 明天 / ≤24h | `#B9761A` | `#F5B544` | `SystemColorWindowTextBrush` |
| `DeadlineWeekBrush` | 本周内 | `#2B6CFF` | `#5B8CFF` | `SystemColorHighlightBrush` |
| `DeadlineFarBrush` | 更远 | `#8A94A6` | `#98A2B3` | `SystemColorWindowTextBrush` |
| `DeadlineNoneBrush` | 未定时间 | `#8A94A6` | `#98A2B3` | `SystemColorWindowTextBrush` |
| `GroupOverdueBrush` / `GroupTodayBrush` | 逾期/今天分组头 | `#B10E1C` | `#FF99A4` | `SystemColorWindowTextBrush` |
| `GroupTomorrowBrush` | 明天分组头 | `#8A5300` | `#F7C565` | `SystemColorWindowTextBrush` |
| `GroupWeekBrush` | 本周内分组头 | `#1B4FD0` | `#8FB0FF` | `SystemColorHighlightBrush` |
| `GroupFarBrush` / `GroupDoneBrush` | 更远/已完成 | `#667085` | `#98A2B3` | `SystemColorWindowTextBrush` |
| `SourceStatusOkBrush` | status ok | `#1A9C5B` | `#3DDC97` | `SystemColorWindowTextBrush` |
| `SourceStatusErrorBrush` | status error | `#D13438` | `#FF6B6B` | `SystemColorWindowTextBrush` |
| `SourceStatusIdleBrush` | status idle | `#8A94A6` | `#98A2B3` | `SystemColorWindowTextBrush` |
| `SourceLoginExpiredBrush` | Cookie 失效 | `#B9761A` | `#F5B544` | `SystemColorWindowTextBrush` |
| `SourceCookieOkBrush` | 已存 Cookie | `#1A9C5B` | `#3DDC97` | `SystemColorWindowTextBrush` |
| `SourceDisabledBrush` | 停用 | `#8A94A6` | `#98A2B3` | `SystemColorWindowTextBrush` |
| `BackendStartingBrush` | 后端启动中 | `#2B6CFF` | `#5B8CFF` | `SystemColorHighlightBrush` |
| `PollingBrush` | 抓取中 | 系统 `AccentFillColorDefaultBrush` | 同 | `SystemColorHighlightBrush` |
| `NetworkErrorBrush` | 断连/源错误 | `#D13438` | `#FF6B6B` | `SystemColorWindowTextBrush` |
| `FirstRunBrush` | 首次运行 | `#2B6CFF` | `#5B8CFF` | `SystemColorHighlightBrush` |
| `NoticeUnreadStripBrush` | 未读卡左 3px 条 | 对应 Score Fill | 对应 Score Fill | `SystemColorWindowTextBrush` |
| `NoticeReadTitleBrush` | 已读标题 | 系统 `TextFillColorPrimaryBrush` | 同 | `SystemColorWindowTextBrush` |
| `NoticeCardBrush` / `NoticeCardHoverBrush` | 卡片底 | 系统 `CardBackgroundFillColorDefaultBrush` / `…SecondaryBrush` | 同 | `SystemColorWindowBrush` |
| `NoticeMetaBrush` | meta 文字 | 系统 `TextFillColorSecondaryBrush` | 同 | `SystemColorWindowTextBrush` |
| `NoticeBodyPanelBrush` | 正文展开区 | 系统 `SubtleFillColorSecondaryBrush` | 同 | `SystemColorWindowBrush` |
| `RemindChipBrush` / `ScoreBadgeBackgroundBrush` | 中性/徽标淡底 | `NeutralSubtleBrush` / 对应档位 Subtle | 同 | `SystemColorWindowBrush` |
| `InfoBarInfoBrush` / `InfoBarSuccessBrush` / `InfoBarWarningBrush` / `InfoBarErrorBrush` | InfoBar 四态 | Info / Success / Warning / Critical **Foreground** | 同 | `SystemColorHighlightBrush` / `SystemColorWindowTextBrush` |

> **系统资源别名（不写 HEX，直接 `ThemeResource` 引用）**：`LayerFillColorDefaultBrush`、`SolidBackgroundFillColorBaseBrush`、`CardBackgroundFillColorDefaultBrush`、`CardBackgroundFillColorSecondaryBrush`、`CardStrokeColorDefaultBrush`、`ControlStrokeColorDefaultBrush`、`DividerStrokeColorDefaultBrush`、`TextFillColorPrimaryBrush`、`TextFillColorSecondaryBrush`、`TextFillColorTertiaryBrush`、`TextFillColorDisabledBrush`、`ControlFillColorDefaultBrush`、`AccentFillColorDefaultBrush` / `Secondary` / `Tertiary`、`FocusStrokeColorOuterBrush` / `Inner`、`SmokeFillColorDefaultBrush`、`SubtleFillColorSecondaryBrush`。参考 HEX 见 2.7，**实现时不硬编码**。

### 10.2 字体 Token

| Token | 值 | Token | 值 |
| --- | --- | --- | --- |
| `FontFamilyPrimary` | `Segoe UI Variable Text`（中文系统回退 `Microsoft YaHei UI`） | `FontIconFamily` | `Segoe Fluent Icons` |
| `FontPageTitle` | 28 / 600 / LH36 | `FontDialogTitle` | 20 / 600 / LH28 |
| `FontSectionTitle` | 14 / 600 / LH20 | `FontCardTitle` | 14 / 600 / LH20 |
| `FontCardTitleRead` | 14 / 400 / LH20 | `FontBody` | 14 / 400 / LH20 |
| `FontBodyStrong` | 14 / 600 / LH20 | `FontCaption` | 12 / 400 / LH16 |
| `FontOverline` | 11 / 500 / LH14 | `FontNumeric` | 14 / 600 / tabular / LH20 |
| `FontNumericLarge` | 20 / 600 / tabular / LH28 | `CharacterSpacingOverline` | ≈20 (1/1000 em) |

### 10.3 间距 / 圆角 / 尺寸 Token

| Token | 值 | Token | 值 |
| --- | --- | --- | --- |
| `Space1` … `Space12` | 4 / 8 / 12 / 16 / 20 / 24 / 32 / 40 / 48（见 5.1 完整映射） | `PagePadding` | Thickness `24,24,24,32` |
| `CompactPagePadding` | Thickness `12,16,12,24` | `CardPadding` | Thickness `16` |
| `CardPaddingDense` | Thickness `12,10,12,10` | `SettingsCardPadding` | Thickness `16,16,20,16` |
| `CardGap` / `SectionGap` | 8 / 24 | `GroupHeaderMargin` | Thickness `0,16,0,8` |
| `ContentMaxWidth` | 940 | `SettingsMaxWidth` | 720 |
| `NavOpenPaneLength` | 240 | `NavCompactPaneLength` | 48 |
| `CommandBarHeight` | 48 | `ButtonMinHeight` / `HitTargetMin` | 32 / 32×32 |
| `RowNoticeMin` / `RowItemMin` / `RowSourceMin` | 96 / 56 / 88 | `RowSettingMin` / `RowInputMin` | 48 / 44 |
| `ChipHeight` / `BadgeHeight` / `NavItemHeight` | 21 / 20 / 40 | `CornerCard` / `CornerControl` / `CornerChip` | 8 / 4 / 4 |
| `CornerFlyout` / `CornerDialog` / `CornerNavIndicator` | 8 / 8 / 2 | `StrokeCard` / `StrokeFocus` | 1 / 2 |
| `StripNotice` / `StripSource` | 3 / 4 | `ElevationCard` / `ElevationFlyout` / `ElevationDialog` | Z=0 / Z=32 / Z=64 |
| `BreakpointCompact` / `BreakpointMinimal` | 640 DIP / 1008 DIP | `IconNav` / `IconInline` / `IconEmpty` | 16 / 14 / 32–40 |

### 10.4 图标 Token（集中常量，码位落地前核对）

`IconRinger` `IconCalendar` `IconGlobe` `IconSetting` `IconRefresh` `IconAdd` `IconFilter` `IconEdit` `IconDelete` `IconCompleted` `IconErrorBadge` `IconClock` `IconWarning` `IconInfo` `IconTag` `IconContact` `IconLink` `IconMore` `IconChevronDown` `IconPlay` `IconSave` `IconDownload` `IconSparkle` `IconPermission` `IconCancel`
> 优先映射到 `Symbol` 枚举；仅 `IconSparkle` / `IconErrorBadge` / `IconSave` / `IconDownload` / `IconPermission` / `IconChevronDown` 等无稳定枚举者用 `FontIcon` 码位，码位统一在 `IconGlyphs.xaml` 定义、落地前核对一次。

---

## 11. 明确避免清单（Anti-patterns · 降低返工）

1. **卡片加投影 / 渐变 / 彩色大色块** —— WinUI 3 靠 1px 描边 + 底色分层，Web 版的 `--shadow` 必须删除。
2. **颜色单独表意** —— 重要度、健康、临期、完成态缺文字或图标即视为缺陷；高对比度下会全废。
3. **覆盖系统 `AccentColor`** —— 主按钮/开关/选中文系统强调色，品牌蓝只管品牌与数据语义。
4. **内容区写死 `#FFFFFF` / `#000000`** —— 破坏 Mica、深色与高对比度。
5. **Mica 与 Acrylic 同层叠加 / 卡片套 Acrylic** —— 三层材质严格分工。
6. **固定 `Height` / 物理像素断点 / 默认行高** —— 150% 缩放必裁中文；一律 `MinHeight` + 显式 `LineHeight` + DIP 断点。
7. **同屏多个 ProgressRing、全屏加载遮罩、刷新闪烁** —— 本地轮询是静默的。
8. **无操作的纯提示吐司用 InfoBar、错误用 TeachingTip、长表单塞进 ContentDialog** —— 各回各位。**例外**：「8s 撤销条」是带 ActionButton 的确认替代，不是瞬时吐司，允许用 InfoBar / 底部实色条（见 §13.3），不算违反本条。
9. **破坏性操作设为默认按钮 / 明文回显凭据** —— 前者误删，后者泄露 DPAPI 保护的账号与 API Key。
10. **自绘托盘菜单、自绘气泡通知、Toast 里塞长文本** —— 系统级 UI 交给系统。
11. **Tab 下划线样式做页内筛选** —— 与左侧顶级导航层级混淆。
12. **任何「开机自启 / 随 Windows 启动 / 后台自启」的开关、文案、图标、引导步骤或跳转列表项** —— 用户硬约束，设置页/托盘菜单/首次运行/跳转列表四处都要检查。

---

## 12. 给下游阶段的交接约束

**给 Phase 2（UX / 交互 / 响应式 / 无障碍）**
- 本文已固定：4/8px 网格、页面 940/720 最大宽、`MinHeight` 行高、三档 DIP 断点（1008 / 640）、导航 `Left/LeftCompact/LeftMinimal` 映射、六类全局状态的唯一载体（顶部 InfoBar + 状态行）、「颜色+图标+文字」三重编码原则。请在此基础上定义交互流程与键盘/讲述人行为，避免另起一套断点或状态位。
- 需你确认/承接：② 重要度筛选阈值 `55 → 50` 的对齐（见 2.4）；② 首次运行 3 步引导的具体触发与跳转；③ `loginExpired` 从 Critical 改判 Warning 后，「重新登录」动作的落点；④ 高对比度下完成态改用删除线+文字（不用 Opacity）的无障碍命名。

**给 Phase 3（前端实现）**
- 直接抄第 10 节 Token 表进 `Themes/`，组件只引用语义别名，代码零裸 HEX。
- 第 6.4 节图标码位**必须**先核对 Segoe Fluent Icons 字形表再定稿；优先用 `Symbol` 枚举。
- `icon.svg` 需转多尺寸 `.ico`（16/20/24/32/48 + @125%/@150% + 深色单色版）供托盘与 PaneHeader 使用。
- 设置页、托盘菜单、跳转列表、首次运行引导四处，逐项核对第 11 节第 12 条（无任何自启项）。
- 中性层用系统 `ThemeResource`，`HighContrast` 分支显式关闭 Mica/Acrylic。

---

## 13. v1.1 增补（对齐 Phase 2 架构侧确认项 A1–A5）

> **触发**：ux-ui-designer 在 Phase 2《体验与交互规范》§10.0 提出 5 项架构侧确认项，并逐行核对了 `server.mjs` / `store.mjs` / `core.mjs` / `notify.mjs` 的真实行为。其中 A1 / A4 / A5 与本文 v1.0 的 §7.13 / §7.8 / §7.10 存在隐含前提、视觉缺口或直接冲突，故增补本章。
>
> **效力**：本章为**权威修订**。下表「原条款」被「新条款」**取代**；Phase 3 遇到冲突时一律以本章为准。**色彩 / 字体 / 间距 / 断点 / 圆角的所有数值一律不变**，本次只增补状态与组件策略。
>
> **边界声明**：A1–A5 本身是**实现架构决策**（Toast 通道、State 存储、进程托管、写队列、延迟提交），不属于视觉设计架构师职责，**最终由 team-lead / 主理人 / Phase 3 拍板**；本章只约束这些决策落地后的**视觉与状态表现**，不代为选择架构方案。

### 13.1 已在正文直接修订的条款

| 位置 | 修订内容 |
| --- | --- |
| §7.3 通知卡·行 5 | 「移除」由「配 ContentDialog 确认」改为「即时乐观移除 + 8s 撤销条，不弹 ContentDialog」 |
| §7.8 设置卡片 | 新增「**在途忙态**（A4）」行：乐观值不回弹 + 邻侧 14px `ProgressRing` + 失败回滚到 InfoBar、成功不弹吐司 |
| §7.10 ContentDialog | 「用途」收窄为仅数据源删除 / 清除账号 / 短表单 / 首次引导；新增「不用 ContentDialog」行明确通知与日程走撤销条 |
| §7.13 原生 Toast | 改为**通道无关**表述；「打开日程」激活按钮标注为仅客户端 `AppNotification` 可用；强制 `reminder.id` 去重防双发 |
| §11 第 8 条 | 澄清：带 ActionButton 的「8s 撤销条」不是瞬时吐司，属允许例外 |

### 13.2 覆盖表（原条款 → 新条款，Phase 3 以此为准）

| 原条款 | 新条款 |
| --- | --- |
| §7.3 避免行 | 追加两条反模式：**「移除」既弹确认框又给撤销**（二选一）；**移除后保留淡出残影**（应立刻消失，防静默刷新闪回） |
| §7.5 筛选项 | 「重要」阈值以主理人拍板的 **score ≥ 50** 为准（与 Web 版 55 为有意差异）。**必须明示「重要：关注度 ≥ 50」，但不得把长文案塞进药丸项**（会撑破 32 高与三项对齐）。落点优先级：① 筛选行下方一行 12px Caption `NoticeMetaBrush`「重要 = 关注度 ≥ 50 的通知」；② 同时写入「重要」项的 `ToolTip` 与 `AutomationProperties.Name`。<640 DIP 紧凑模式可只保留 ② |
| §7.6 日程行·操作 | 「删除」同样走「即时移除 + 8s 撤销条」，**不弹 ContentDialog**。`item.add` 返回 id 时可用 `item.remove{id}` 做真撤销，视觉表现与延迟提交撤销一致 |
| §7.7 数据源·右侧 | 删除数据源**仍保留 ContentDialog 确认**（不可逆、连带登录态配置），**不**走 8s 撤销条。这是与通知/日程移除唯一的确认策略差异，实现须显式区分 |
| §7.9 InfoBar | 增补「撤销反馈（独立于本体系）」行：**8s 撤销条**可用 `InfoBar`（`Severity=Success`、`IsClosable=True`、`ActionButton=撤销`）或独立底部实色条；它**独立于全局状态位体系（非例外、非第 5 种状态）**，因此与顶部常驻 Error/Warning 一上一下并存（不得叠在同一位置），**不受同一时刻最多 1 条的择一逻辑影响**。该行已同步落进 §7.9 正文表格，覆盖表与正文自洽；纯成功吐司仍沿用原 Success 态 |
| §7.12 后端启动中 | 若采用 A3「先 GET /api/health 命中健康服务则复用、不 spawn」，**复用路径不得再显示「正在连接本地服务…」**，直接进正常态；否则会闪一下被误判断连。仅真正等待本实例 spawn 时才显示该态 |
| §12 交接约束 | 由 §13.6 取代（Phase 2 的 4 项承接已完成，新增 A1–A5 视觉约束） |

### 13.3 新增组件规范 7.14 · 撤销条（Undo Bar）

> 逻辑上属于 §7 组件章，因 v1.1 增补记于此。它是 A5「8s 延迟提交」机制**唯一**的视觉载体，替代通知/日程移除的 ContentDialog。
>
> **适用范围（主理人裁定 · Phase 3 权威依据）**：**本组件仅适用于「移除型延迟提交」一种语义**；`item.add` 等「已提交且有反向 op（如 `item.remove{id}`）」的场景**不适用**。这类场景采用方案 1：通知页「一键加入日程」用 §7.9 Success InfoBar（自动 4–6s、可关闭）+ state 刷新；日程页「手动添加日程」不弹任何提示，只靠列表与 `InfoBadge` 计数变化；用户反悔到日程页删除该条目，删除本身由本组件兜底。**不引入「新增型」语义变体**，避免组件语义膨胀。

| 项 | 规定 |
| --- | --- |
| 位置 | 内容区**底部居中**，最大宽 480，距底边 24；不遮挡列表最后一行（列表底部额外留 56 空白）；不覆盖命令栏与顶部 InfoBar |
| 容器 | 不透明 `CardBackgroundFillColorDefaultBrush` + 1px `CardStrokeColorDefaultBrush`，圆角 8，**Z=0 无投影**（与卡片一致）；**不用 Acrylic**（常驻 8s 的面板不用瞬态材质） |
| 左侧 | 16px `Delete` 图标（`NeutralForegroundBrush`，**不用红色**，避免被误读为错误）+ 14px `FontBody`「已移除「{标题前 20 字}…」」单行省略 |
| 右侧 | 「撤销」`DefaultButtonStyle`（高 28–32，`MinWidth = 64`）+ 12px `FontNumeric` tabular 倒计时「8s…1s」右对齐 |
| 倒计时 | 底部 2px 确定性 `ProgressBar`（`NeutralFillBrush`）8s 线性递减；**不用环形计时器**，避免与 §7.12 的 ProgressRing 冲突 |
| 层级 | 浮于列表之上但不抢焦点：无模态遮罩、不夺键盘焦点、无滑入音效；靠 1px 描边与列表分隔，不额外加 `ThemeShadow` |
| 多条目 | 8s 内连续移除多条 → 只保留**最新 1 条**，文案变「已移除 N 项」，撤销一次性恢复全部；**不堆叠多条** |
| 提交 / 撤销 | 倒计时归零 → 条消失并发出移除请求；点「撤销」→ 条立即消失、卡片**立刻回到原位**（不做淡入动画）。窗口退出前必须 flush 待删除集合，并在 12px 状态行给出提示（**不弹窗**） |
| 静默刷新 | 10s 轮询与 8s 窗口重叠时，待删除集合必须过滤 state，被移除卡片**不得闪回**；过滤期间列表不出现插入/删除动画（延续 §7.12「不加闪烁」） |
| 无障碍 | `AutomationProperties.Name`「已移除 1 项，8 秒内可撤销」；「撤销」按钮 Tab 可达；高对比度下靠文字与按钮，不靠倒计时颜色 |
| 避免 | 做成模态/全屏遮罩、用 Success 绿庆祝删除、用红色警示、堆叠多条、撤销后做淡入动画、8s 内又弹 Success InfoBar、把删除数据源也走此条（应走 ContentDialog） |

### 13.4 A1–A5 的视觉侧约束（架构决策本身不在此拍板）

| 项 | 视觉侧约束（无论架构怎么选都必须满足） |
| --- | --- |
| **A1 Toast 单一通道** | 见修订后 §7.13：视觉通道无关、系统托管、≤2 行、强调色跟随系统、不自绘、按 `reminder.id` 去重**严禁双发**。若选客户端 `AppNotification` → 可有「打开日程」按钮；若选后端托管 → 该按钮取消，改由点击 Toast 本体 + 客户端激活事件跳转。**两种通道的视觉差异仅此一处** |
| **A2 单一 State** | 「失败也必须应用 state」与视觉一致：`source.status='error'` / `lastLogin` 等部分变更必须即时反映到数据源 4px 状态条与顶部 InfoBar；静默 10s 刷新在写操作在途暂停合并，对应 §7.12「不加遮罩、不加闪烁」。全局单一版本源可避免列表数字与徽标计数互相打架 |
| **A3 server 托管** | 复用健康服务时不显示启动态（见 §13.2 / §7.12）；「谁启动谁关闭」可避免误杀他实例服务而把用户置入 §7.9 的常驻断连 Error 态（该态视觉很重，应为真实故障保留） |
| **A4 写队列** | 见修订后 §7.8「在途忙态」5 条；表单类显式一次性保存与本文原设计一致，无需改数值 |
| **A5 延迟提交** | 见 §7.3 / §7.6 / §7.10 / §13.3：通知与日程走撤销条、数据源与清除账号走 ContentDialog；静默刷新过滤待删除集合防闪回 |

### 13.5 主理人已拍板项的视觉落点

| 拍板项 | 视觉落点 |
| --- | --- |
| 「重要」筛选阈值 `score ≥ 50`（与 Web 版 55 有意差异） | 本文 §2.4 已默认按 50 设计徽标与筛选样式，**色板与阈值数值无需改动**；明示文案落点见 §13.2 / §7.5（筛选行 12px Caption + ToolTip + 无障碍名，不塞进按钮） |
| `loginExpired` 恒为 Warning 橙 + 必有「重新登录」入口 | 与本文 §2.6 / §7.7 完全一致（Warning chip + `Warning` 图标 + 同排「重新登录」文字按钮），**无需改动**；已配置 → `account.test`、未配置 → 设置页 `account.save` 的跳转由 Phase 2 定义，视觉上按钮不变 |
| 四处零开机自启 / 凭据 PasswordBox 不回显 | 与 §7.8 / §7.13 / §11-12 一致，确认无遗漏 |

### 13.6 更新后的交接约束（取代 §12）

**给 Phase 2**：4/8px 网格、940/720 最大宽、`MinHeight` 行高、三档 DIP 断点（1008 / 640）、导航映射、六类全局状态唯一载体、三重编码原则均已固定且**未变**；阈值 50 / 首次引导 / 重新登录落点 / 完成态无障碍命名 4 项已由 Phase 2 承接完成（附录 C 登记），本文确认一致。请在你方规范中同步 §13.2 覆盖表涉及的确认策略分叉（撤销条 vs ContentDialog）与 §7.8 在途忙态，避免两份规范对同一交互给出不同描述。

**给 Phase 3**：除 §12 原有 5 条外，追加：
- **确认策略分叉**：通知/日程移除 = 8s 撤销条（§13.3）；数据源删除 / 清除已保存账号 = ContentDialog（§7.10）。**不要**给可撤销操作再弹确认框。
- **Toast 通道**：按 A1 拍板结果二选一，按 `reminder.id` 去重；仅客户端通道可带「打开日程」激活按钮。
- **写队列忙态**：4 个行为开关实现 §7.8「在途忙态」5 条；表单类显式保存。
- **启动态时序**：复用健康服务时不显示「正在连接本地服务…」。
- **刷新过滤**：静默刷新过滤待删除集合，防卡片闪回；失败也必须应用 state。
- 本文所有色彩 / 字体 / 间距 / 圆角 / 断点数值 **v1.0 → v1.1 未变**，§10 Token 表可直接照抄。

### 13.7 输入框错误态（表单显式保存失败的视觉补完）

> **缺口性质说明**：本节是对 Phase 2《体验与交互规范》§9.1「表单显式保存失败 = 字段内联错误」的**视觉补完**。本文 §7.8 只规定了「开关即时保存失败 → 顶部 InfoBar Error」与「在途忙态」，**未覆盖表单类失败**，属真实规格缺口，**不是对 v1.0 / v1.1 既有条款的修订冲突**。本节不新增任何色彩 / 字体 / 间距 / 断点 / 圆角数值，全部复用 §10 Token 表已有 Token；§7.8 原文保持不变。

| 项 | 规定 |
| --- | --- |
| 适用控件 | `TextBox` / `NumberBox` / `PasswordBox`：设置页表单（抓取间隔、关注关键词、提醒档位、CAS 入口/回调地址、学号、API Key、CAS 密码）与短表单（标题 / 时间 / URL） |
| 错误态描边 | **1px `CriticalFillBrush`**，替换默认 `ControlStrokeColorDefaultBrush`；圆角仍 4（`CornerControl`），**不**加粗到 2px |
| 错误文案 | 控件**下方** 12px `FontCaption`，色 `CriticalForegroundBrush`；`TextWrapping = Wrap`，最多 2 行；与控件间距 4（`Space1`） |
| 错误图标 | 文案前置 14px `ErrorBadge` 图标（`IconInline`），色 `CriticalForegroundBrush`；图标与文字间距 4 |
| 焦点框 | 错误字段的键盘焦点框仍用 2px 系统 `FocusStrokeColor*`，**不用** Critical 色覆盖焦点框（焦点与错误是两套独立视觉信号） |
| 无障碍 | `AutomationProperties.Name` 与 `AutomationProperties.HelpText` 均带错误说明（如「抓取间隔，最小 5 分钟，当前值 2 无效」）；高对比度下**靠图标 + 文字**，不靠描边颜色 |
| 状态恢复 | 错误解除后恢复默认描边与默认文案位；不新增 Success 提示，由「保存设置」的 §7.9 Success InfoBar 统一承担 |
| **两类失败必须显式区分** | ① **开关即时保存失败** → §7.8「在途忙态」④：回滚乐观值 + **顶部 InfoBar Error**；② **表单显式保存失败** → **字段内联错误**（本节）。反馈位置不同，**不得混用**：表单失败不升级到顶部 InfoBar，开关失败不落到字段内联 |
| 避免 | 整框红底 / 淡红填充、用 `ToolTip` 藏错误（看不见即无障碍缺陷）、弹窗或 ContentDialog、在输入框内嵌错误文字、用 Success 色表达「已修复」、把整组设置卡染成 Critical 色 |

