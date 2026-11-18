# 校园通知 · Rust 核心 + Tauri 2 双端改造方案

> 适用基线：`main @ 461e424`（`core.mjs` 1031 行 / `store.mjs` 1114 行 / `server.mjs` 426 行 / `public/app.js` 1023 行）。
> 本文是实施规格，不是愿景文档：每条接口、每个状态、每个阈值都可对照现有代码或可测。
> 关联文档：原 WinUI 交互文档已随旧版移除；op 映射与交互口径并入本文 §3.3 / §10.3。
> 基线文件在迁移完成后已从仓库删除，行号引用仅作历史记录；可回滚标签 `backup/pre-rust-reset`。

---

## 1. 目标与非目标

### 1.1 目标

| # | 目标 | 可验证结果 |
| --- | --- | --- |
| G1 | 业务逻辑收进跨平台 Rust 核心 | 同一份 `cn-*` crate 编译到 Windows 与 Android，行为一致 |
| G2 | 双端外壳共用一套 UI | 同一份前端产物在 WebView2 与 Android System WebView 渲染 |
| G3 | 登录流程可交互、可解释、可恢复 | 11 状态机 + 11 个稳定错误码，每码有文案与下一步动作 |
| G4 | 会话自动保持 | 重启免重登；过期后单次静默续期，无需用户干预 |
| G5 | LLM 承担摘要 / 分类 / 结构化抽取 | 输出经本地 Schema 校验，失败可修复、最终可降级 |
| G6 | 体积与性能达标 | Windows 安装包 ≤ 8 MB，APK（arm64）≤ 12 MB，冷启动 ≤ 1 s |

### 1.2 非目标

- 不改产品模型：仍是"本机抓取 + 本机存储 + 本机提醒"，不做云端账号体系。
- 不保留 `public/app.js` 的 DOM 结构作为约束；保留其信息架构与交互语义。
- 不做 iOS（本期）；不做多用户。
- 不追求 Android 秒级提醒（系统限制，见 §8）。

---

## 2. 模块边界

### 2.1 工作区结构

```
campus-notice/
├─ Cargo.toml                  # [workspace] members = crates/*, apps/app/src-tauri
├─ crates/
│  ├─ cn-core/                 # 纯逻辑：解析 / 抽取 / 打分 / 规则摘要（零 IO、零网络、零 async）
│  ├─ cn-net/                  # HTTP / 编码 / Cookie 罐 / DNS / WebVPN / CAS 登录
│  ├─ cn-store/                # 持久化 + 抓取流水线 + 日程 + 提醒调度
│  ├─ cn-llm/                  # OpenAI 兼容客户端 + Schema 校验 + 缓存
│  └─ cn-model/                # 跨 crate 共享 DTO / 错误码 / 配置结构（serde）
├─ apps/app/
│  ├─ src-tauri/               # 唯一允许 use tauri:: 的地方（命令层 + 平台外壳）
│  ├─ gen/android/             # tauri android init 生成
│  └─ ui/                      # 前端（自 public/ 迁移）
├─ fixtures/                   # 录制的真实响应 + 期望输出（见 §9 P0）
└─ tools/golden.mjs            # 用现有 Node 实现导出期望值的脚本
```

### 2.2 依赖方向（强制，CI 校验）

```
cn-model  ←  cn-core  ←  cn-net  ←  cn-store  ←  src-tauri
                ↑          ↑           ↑
             cn-llm ───────┘           │
                                        └── 只有 src-tauri 依赖 tauri / tokio runtime
```

四条硬规则：

1. `cn-core` 的 `Cargo.toml` **不得**出现 `reqwest` / `tokio` / `tauri`；CI 用 `cargo tree -p cn-core` 断言。
2. `cn-*` 全部 crate **不得**依赖 `tauri`；平台能力通过 `cn-model` 里的 trait 注入。
3. `cn-core` 不读时钟、不读文件、不读环境变量：所有时间参数显式传入 `&Clock`，保证 golden test 可复现。
4. 前端不得直接发网络请求（含 LLM）；一切出站经命令层，便于统一脱敏与限流。

### 2.3 各 crate 职责

| crate | 负责 | 明确不负责 |
| --- | --- | --- |
| `cn-core` | HTML/RSS/JSON 列表解析、正文定位、截止时间与联系人抽取、打分、规则摘要、日期归一 | 发请求、落盘、日志 |
| `cn-net` | 请求构造、重定向、Cookie 罐、编码嗅探、解压、DNS 覆盖/DoH、WebVPN 编解码、CAS 表单登录 | 业务字段判断、持久化 |
| `cn-store` | `Store` 状态、18 个 op、去重、抓取流水线编排、日程、提醒引擎、原子落盘 | UI、网络细节 |
| `cn-llm` | 提示词构造、批量调用、Schema 校验、修复重试、结果缓存、配额 | 决定"哪些通知值得发"（由 `cn-store` 决定） |
| `src-tauri` | 23 个命令、事件广播、托盘/通知/自启动/Keystore、Android WorkManager 桥 | 任何业务规则 |

---

## 3. 核心接口

### 3.1 `cn-core`：纯函数契约

```rust
pub struct RawItem {
    pub item_id: String,
    pub title: String,
    pub url: String,
    pub published_at: String,   // YYYY-MM-DD 或空
    pub context: String,        // 列表页碎片 / 接口摘要
}

pub struct SourceSpec {          // 与 store.json 的 source 字段一一对应
    pub id: String,
    pub kind: SourceKind,        // Html | Rss | Json
    pub url: String,
    pub link_pattern: Option<String>,
    pub items_path: Option<String>,
    pub map: FieldMap,           // title/url/id/date/summary
    pub url_template: Option<String>,
    pub url_template_kind: TemplateKind, // Id | Index | Title
}

pub fn parse_html_list(html: &str, base: &Url, spec: &SourceSpec) -> Vec<RawItem>;
pub fn parse_rss(xml: &str, base: &Url) -> Vec<RawItem>;
pub fn parse_json_list(body: &str, spec: &SourceSpec) -> Vec<RawItem>;

pub struct Extract { pub tags: Vec<String>, pub deadline: Option<Deadline>, pub contact: Option<String> }
pub fn extract(title: &str, body: &str, keywords: &[String], now: &Clock) -> Extract;
pub fn score(input: &ScoreInput) -> u8;          // 22 起算，5..=98 夹紧
pub fn rule_summary(input: &SummaryInput) -> RuleSummary;
pub fn normalize_due(raw: &str) -> Option<LocalDateTime>;
pub fn clean_title(raw: &str) -> String;
pub fn strip_red_header(text: &str) -> String;
```

实现约束：

- **不得**使用带向前/向后断言的正则（Rust `regex` 不支持）。`find_date` 中的 `(?![\d.])` 必须改写为"匹配后检查下一字符"或改用 `fancy-regex`；后者只允许出现在 `cn-core` 的 `regex_compat` 模块，便于替换。
- **时钟注入**：`now: &Clock` 是唯一时间来源；禁止 `SystemTime::now()`。

### 3.2 `cn-net`：网络与认证契约

```rust
#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, req: FetchRequest) -> Result<FetchResponse, NetError>;
}

pub struct FetchRequest {
    pub url: Url,
    pub method: Method,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
    pub scope: ScopeId,              // 决定用哪个 Cookie 罐 + 哪个网关配置
    pub timeout: Duration,
    pub max_bytes: usize,            // 默认 6 MiB，与现状一致
    pub follow_redirects: u8,        // fetchRaw=4 / jarRequest=8
    pub resolve_ip: Option<IpAddr>,
    pub allow_invalid_cert: bool,    // 仅该源显式开启
}

pub trait SessionProvider {
    async fn ensure(&self, scope: &ScopeId) -> Result<CookieHeader, AuthError>;
    async fn invalidate(&self, scope: &ScopeId);
    fn state(&self, scope: &ScopeId) -> SessionState;
}

pub trait GatewayCodec {             // WebVPN
    fn rewrite(&self, url: &Url) -> Url;
    fn restore(&self, url: &str) -> String;
    fn restore_in_text(&self, text: &str) -> String;
    fn detect_login_page(&self, body: &str) -> bool;
}
```

`ScopeId` 的语义：`Gateway(host)` 与 `Portal(host)` 是两个独立罐。现状已把网关 Cookie 与门户 Cookie 分开（`settings.webvpn.cookie` vs `source.cookie`），改造后升级为"每 scope 一个罐 + 一个凭据条目"，仍然不混发。

### 3.3 `cn-store`：18 个 op 保持不变

**关键决策：不重新设计 op 契约。** `act(op, payload) -> ActionResult` 是现有 23 个 op 的超集，保留它带来三个好处：前端可增量迁移、Node 版可作为长期对照实现、本文 §10.3 的行为保留清单不失效。

```rust
#[derive(Deserialize)]
pub struct ActRequest { pub op: String, #[serde(flatten)] pub payload: serde_json::Value }

#[derive(Serialize)]
pub struct ActionResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub preview: Option<Vec<PreviewItem>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub id: Option<String>,
    pub state: StateDto,             // 成功与失败都必须带
}
```

`StateDto` 字段与 `store.mjs:1025-1078` 逐项对齐，包含：`settings`（`webvpn` 只暴露 `enabled/gateway/hasCookie`）、`sources`（`cookie` 换成 `hasCookie`、含 `loginExpired`）、`notices[0..200]`、`items[0..300]`、`meta`（`reminders` 取前 20、`account.configured`、`polling`、`llmKeySet`）。

### 3.4 命令层（Tauri）

| 命令 | 入参 | 出参 | 说明 |
| --- | --- | --- | --- |
| `get_state` | — | `StateDto` | 启动与重连用 |
| `act` | `{op, payload}` | `ActionResult` | 23 个 op 的统一入口 |
| `auth_begin` | `{scope, mode}` | `AuthTicket` | `mode = WebView | Password | ImportCookie` |
| `auth_cancel` | `{ticket}` | `()` | 用户中途取消 |
| `auth_import_cookie` | `{scope, cookie}` | `ActionResult` | 桌面兜底路径 |
| `llm_test` | — | `TestResult` | 不覆盖已存 Key |
| `export_data` | `{path}` | `()` | 导出 `store.json`（不含密钥） |
| `import_data` | `{path}` | `ActionResult` | 迁移自 Node 版 |

事件（前端 `listen`）：

| 事件 | 载荷 | 频率 |
| --- | --- | --- |
| `state://changed` | `{version, state, origin}` | 每次 op 或静默刷新 |
| `auth://progress` | `{scope, state, step, hint, at}` | 登录过程中，状态变化即发 |
| `poll://progress` | `{sourceId, done, total, phase}` | 每源每阶段 |
| `reminder://fired` | `{id, itemId, title, note, dueAt}` | 到点触发 |
| `log://line` | `{level, text}` | 脱敏后的运行日志 |

**版本号**：`state.db` 与内存各持单调 `version`，前端只应用更大版本，避免静默刷新覆盖写结果（沿用 `StateStore` 的既有设计）。

---

## 4. 数据流

### 4.1 抓取流水线（一次 poll）

```
触发（定时 / 手动 / 开机补跑）
  │
  ├─ 0. 取启用源（enabled && url 非空）；为空 → lastPollError="没有启用任何数据源"
  │
  ├─ 1. 逐源（串行，源间 sleep 250 ms）
  │     ├─ ensure(scope)  ── 会话无效则走 §5 状态机（单飞）
  │     ├─ fetch(列表页 × pages，按 p={n} 翻页)
  │     ├─ decodeBody（charset 嗅探 → GBK/UTF-8）
  │     ├─ 网关登录页检测（仅 WebVPN 开启时）
  │     ├─ restore_urls_in_text（把网关地址还原成门户地址）
  │     └─ parse_html_list / parse_rss / parse_json_list
  │
  ├─ 2. 截断 maxItemsPerSource（默认 100，范围 1–200）
  ├─ 3. 去重：dedupeKey = "{sourceId}|{sha1(cleanTitle)[0..16]}"
  ├─ 4. 登录失败重试：can_auto_login 时最多 2 轮，只重跑 loginExpired 的源
  │
  ├─ 5. 详情跟进（可选）：按"标题命中关键词数"排序，取 detailLimit（10–40，默认 25）；
  │     每篇 sleep 120 ms；正文走 strip_red_header
  │
  ├─ 6. LLM 批量（可选）：按关键词命中排序取 llmMaxPerPoll（默认 8，1–40 条）
  ├─ 7. 逐条补全：tags / deadline / contact / summary / useful / relevant / score
  │     有 AI 结果用 AI，无则全部落规则（永不空白）
  │
  ├─ 8. 排序：publishedAt 倒序，同日按 score 倒序；并入 store 头部
  ├─ 9. 裁剪：notices 400 条、items 600 条、meta.reminders 40 条
  ├─ 10. 自动建日程（可选）：有 deadline、relevant≠false、due ≥ now-1h、且同 noticeId 未建过
  ├─ 11. 写 meta.lastPollAt / pollCount / lastPollError（聚合失败源）
  └─ 12. 原子落盘（临时文件 + rename）→ 广播 state://changed
```

并发约束：全局并发 ≤ 2；同 host 最小间隔 300 ms；单次 poll 总超时 5 min；超时按"部分成功"返回，已入库的数据不回滚。

### 4.2 提醒引擎

- 心跳 15 s（前端在 `App` 的定时器，不依赖 UI 是否可见）。
- 判定：`now ≥ due - level` 且 `now ≤ due + 5 min` 且 `level ∉ fired[]`。
- 命中即写 `fired[]`、生成 `meta.reminders` 条目（前 40）、发系统通知。
- 启动时立即跑一次（补上停机期间窗口内的提醒），再进入 15 s 周期。
- 跨设备时钟漂移：以"单调时钟 + 每次唤醒校准"处理，不用单纯 `Instant` 累加。

### 4.3 LLM 结构化流

```
notice(title + content[0..900])
   → 批量打包（i, title, date, content）
   → POST {baseURL}/chat/completions  (temperature 0.2, max_tokens 2400)
   → 提取首个 '[' 到末个 ']' 的片段
   → serde_json 解析 → Schema 校验
        ├─ 通过 → 写回 notice（category/tags/deadline/summary/useful/relevant）
        └─ 失败 → 带"校验错误"重试 1 次
                   └─ 再失败 → 整批降级 rule_summary / pick_tags / find_deadline
   → 缓存：ai_hash = sha1(title + content[0..2000] + model + PROMPT_VERSION)
```

---

## 5. 交互式登录流程

### 5.1 三种模式与优先级

| 模式 | 适用 | 双端支持 | 说明 |
| --- | --- | --- | --- |
| **WebView 登录**（推荐） | 有验证码 / 短信 / 二次验证 | 桌面 + Android | 应用内打开登录页，用户完成交互，导航回 `service` 后自动取 Cookie |
| **账号密码**（CAS 表单） | 无验证码的统一身份认证 | 桌面 + Android | 用于"会话过期后的静默续期"，不替代首次登录 |
| **粘贴 Cookie**（兜底） | 任何异常场景 | 仅桌面 | 保留现有路径，移动端可用"从剪贴板导入" |

优先级：首次登录走 WebView（保证成功率），续期走账号密码（保证无感）；两者都失败才提示手动 Cookie。

### 5.2 状态机（7 主态 + 4 异常态）

```
Unconfigured ──configure──▶ Ready
Ready ──begin──▶ Launching ──page_loaded──▶ AwaitingUser
AwaitingUser ──navigate_back──▶ Verifying ──cookie_ok──▶ Active
AwaitingUser ──user_cancel──▶ Cancelled
Launching/AwaitingUser ──fail──▶ Failed{kind}
Verifying ──fail──▶ Failed{kind}
Active ──probe_fail / 401 / login_page──▶ Expired
Expired ──auto_renew_ok──▶ Active
Expired ──auto_renew_fail──▶ Failed{kind}
Active ──now > last_verified + 8h 且无 Max-Age──▶ Expiring ──probe──▶ Active | Expired
Failed ──user_retry──▶ Ready
Blocked{captcha} ──switch_to_webview──▶ Launching
```

状态到 UI 的映射（每态必须有：标题 / 说明 / 主操作 / 是否可取消）：

| 状态 | 标题 | 说明 | 主操作 |
| --- | --- | --- | --- |
| `Unconfigured` | 还没登录学校门户 | 登录后才会抓取到通知 | 去登录 |
| `Ready` | 准备登录 | 选择登录方式（窗口登录 / 账号密码） | 开始 |
| `Launching` | 正在打开登录页 | 若 5 s 未加载，可改用粘贴 Cookie | 取消 |
| `AwaitingUser` | 请在窗口中完成登录 | 需要验证码/短信时也在这里完成 | 我已完成 / 取消 |
| `Verifying` | 正在确认登录状态 | 校验门户是否下发了会话 Cookie | —（不可取消） |
| `Active` | 已登录 · 有效期至 … | 下次抓取将自动使用 | 重新登录 |
| `Expiring` | 登录即将过期 | 已提前续期，无需操作 | 立即续期 |
| `Expired` | 登录已过期 | 正在自动重登 | 手动登录 |
| `Failed` | 登录失败：{kind} | 见 §5.3 的下一步动作 | 重试 / 换方式 |
| `Cancelled` | 已取消登录 | 不影响已抓到的通知 | 重新登录 |
| `Blocked` | 需要人工验证 | 出现验证码，自动登录不可用 | 改用窗口登录 |

**反馈通道**：每次状态变化 emit `auth://progress`，载荷含 `step_index/step_total`，前端渲染为 3 步指示（打开登录页 → 完成验证 → 确认会话）。Android 上额外做 500 ms 轮询兜底，防止 WebView 后台事件丢失。

### 5.3 错误码与下一步动作

| 码 | 触发条件（对应现状） | 用户文案要点 | 下一步 |
| --- | --- | --- | --- |
| `AUTH_E_NETWORK` | DNS 失败 / 超时 / 连接拒绝 | 无法连接学校门户 | 检查是否在校内网；一键开启 WebVPN |
| `AUTH_E_TLS` | 证书链错误 | 证书不受信任 | 展示指纹，需显式信任（仅该源） |
| `AUTH_E_CREDENTIAL` | 停留在 `/login` 且有 errorDiv | 账号或密码不对 | 重填；连续 3 次转 WebView |
| `AUTH_E_CAPTCHA` | 检测到验证码输入框且可见 | 需要验证码 | 转 WebView 登录 |
| `AUTH_E_MFA` | 出现短信/动态码字段 | 需要二次验证 | 转 WebView 登录 |
| `AUTH_E_FORM` | 找不到 `execution` | 登录页结构变了 | 转 WebView；附诊断信息 |
| `AUTH_E_NO_PORTAL_SESSION` | 只拿到 CAS Cookie | 登录成功但门户没给会话 | 自动尝试 2 个候选回调地址，再引导改 `service` |
| `AUTH_E_GATEWAY` | 200 + 登录页 / 网关 JSON 错误体 | WebVPN 会话过期 | 重新登录网关，更新网关 Cookie |
| `AUTH_E_PORTAL_BIZ` | `e=10041/10042` 或"请登录" | 门户登录态失效 | 触发续期；失败转 WebView |
| `AUTH_E_CANCELLED` | 用户取消 | 已取消 | 可重试 |
| `AUTH_E_INTERNAL` | 其他 | 未知错误 | 导出诊断包 |

所有错误对象统一为 `{code, message, detail, retryable, next_action}`，禁止把后端英文异常直接抛给 UI。

### 5.4 会话保持

1. **持久化**：Cookie 罐序列化为 `sessions.json.enc`，密钥存 OS 密钥库（Windows Credential Manager / Android Keystore）。文件里只有密文与元数据：`scope / saved_at / last_verified_at / expires_at(可空)`。
2. **主动探测**：每次 poll 前对每个 scope 发一个最轻请求（列表第 1 页，`pages=1`），失败才触发续期；避免"每次抓取都登录"。
3. **单飞**：同一 scope 的并发 `ensure()` 共享一个 `OnceCell<Result<..>>`，10 s 内不重复登录。
4. **退避**：续期失败按 1 min → 5 min → 15 min → 1 h 退避，1 h 内最多 4 次；连续 3 次失败置 `Failed` 并停自动重试，等用户操作。
5. **过期预测**：`Set-Cookie` 带 `Max-Age/Expires` 用真值；会话 Cookie（无这两个属性）用 `last_verified + 8h` 估计，进入 `Expiring` 并提前续期。
6. **双端一致性**：Android 进程被杀后从加密罐恢复，不要求重登；Keystore 密钥失效（卸载重装/恢复出厂）→ `Unconfigured`，明确提示"需重新登录"。
7. **隔离**：网关罐与门户罐永不合并；日志中 Cookie 只输出 `name` 与长度，不输出值。

---

## 6. LLM 能力集成

> 端侧检索增强（RAG）见 **`docs/rag-on-device-design.md`**：索引管线、混合检索、三档路由、端侧运行时选型与评估口径。
> 本节只描述「单批调用」的摘要 / 分类 / 结构化抽取；RAG 的生成走同一个 `LlmBackend`，只是上下文来源不同。

### 6.1 三类任务合并为一次调用

现状是一次批量调用完成摘要，改造后同一次调用额外产出分类与结构字段，避免三次往返：

```json
[
  {
    "i": 0,
    "category": "教务 | 考试 | 评奖资助 | 讲座学术 | 就业实习 | 后勤生活 | 招标采购 | 其他",
    "gist": "一句话讲清这件事，不超过 60 字",
    "useful": ["对我有用的点，每条不超过 30 字，最多 3 条"],
    "tags": ["2-4 个标签"],
    "deadline": "YYYY-MM-DDTHH:mm 或空字符串",
    "contact": "联系人与电话，没有则空",
    "audience": "本科生 | 研究生 | 全体 | 教师 | 不限",
    "relevant": true,
    "confidence": 0.0
  }
]
```

分类是新增能力，直接支撑 UI：通知页新增"按类别筛选"，日程页可按类别着色。类别是**封闭枚举**，模型输出越界一律归"其他"并计入指标（`category_fallback_rate`）。

### 6.2 校验与修复

| 校验项 | 规则 | 失败处理 |
| --- | --- | --- |
| JSON 可解析 | 截取首个 `[` 到末个 `]` | 重试 1 次 |
| 索引合法 | `i` 在 `0..batch.len()` | 丢弃该条 |
| 字符串长度 | gist ≤ 120 / useful ≤ 4 条每条 ≤ 60 / tags ≤ 5 | 截断 |
| 枚举 | `category` 必须在 8 项内 | 归"其他" |
| 时间 | 正则 + 年份 2000–2100 + 不早于发布日 -1 天 | 置空，回落 `find_deadline` |
| 置信度 | `0.0–1.0` | 缺失按 0.5 |

**不依赖服务端强约束**：OpenAI 兼容端点对 `json_schema` 的支持不一致（DeepSeek 目前稳定支持的是 `json_object`），所以采用"提示词约束 + 本地严格校验 + 一次修复"，而不是赌服务端。

### 6.3 配额、缓存与降级

- 单条正文截断 900 字（沿用现状），批量上限 `llmMaxPerPoll`（默认 8）。
- 缓存键含 `PROMPT_VERSION`，换提示词自动失效；`notices.rescore` 命中缓存不发请求。
- 月度 token 预算（可配），超限自动只走规则摘要，并在设置页显示"本月已用 / 上限"。
- 降级顺序：AI → 规则（`rule_summary` / `pick_tags` / `find_deadline` / `find_contact`）→ 空。**任何情况下通知都必须有摘要与"对我有什么用"**，这是现状的硬约束，必须保留。

### 6.4 隐私边界

- 通知正文会离开设备，首次开启 LLM 时需一次显式同意，文案说明"正文将发送至 {baseURL}"。
- 每个数据源新增 `sendToLlm: bool`（默认 true），学籍/成绩等敏感源可单独关闭。
- 日志与诊断包不含正文；只记录 token 数与耗时。
- `baseURL` 变更需二次确认，防止配置被改成外发地址。

---

## 7. 配置与安全

### 7.1 配置分层

| 层 | 位置 | 内容 | 是否加密 |
| --- | --- | --- | --- |
| 数据 | `{app_data}/store.json` | 通知、日程、数据源定义（不含 Cookie）、settings | 否（用户可读可备份） |
| 凭据 | OS 密钥库 + `sessions.json.enc` | CAS 密码、LLM Key、各 scope Cookie 罐 | 是 |
| 应用配置 | `{app_config}/app.json` | 端口、`llm.baseURL/model`、预算、`pollMinutes` | 否 |
| UI 偏好 | `{app_config}/prefs.json` | 筛选、上次页面、引导完成标记 | 否 |

与现状的差异：现在的 `config.json` 同时装"配置 + DPAPI 密码 + 明文 API Key"，改造后**配置与凭据物理分离**，删数据不必担心误删凭据、备份配置不会泄露密码。

### 7.2 安全要求（逐条可验）

| # | 要求 | 验证方式 |
| --- | --- | --- |
| S1 | 密钥只存 OS 密钥库，磁盘无明文 | 全盘 grep `sk-` / 密码字段；`strings` 抽查二进制 |
| S2 | 凭据不回传前端，只回 `hasCookie/hasKey/configured` | 抓 `get_state` 响应断言无敏感字段 |
| S3 | 日志脱敏：`cookie/authorization/api_key/password` 值替换为 `***` | 单测喂样例串，断言输出 |
| S4 | CSP 严格：`default-src 'self'`，禁远程脚本与内联 eval | `tauri.conf.json` 的 `csp` + 构建后检查 |
| S5 | Tauri 能力最小化：capabilities 只开 notification/opener/fs(scope) | 审计 `capabilities/*.json` |
| S6 | SSRF：允许内网（业务需要），禁止 `file/ftp/data` 与 link-local `169.254.0.0/16` | 单测 12 个 URL 用例 |
| S7 | 证书：默认严格校验；`allow_invalid_cert` 仅按源开启且 UI 明示 | 单测 + 手工验证 |
| S8 | Android：`exported=false`、`allowBackup=false`、`usesCleartextTraffic=false` | 检查合并后的 Manifest |
| S9 | 依赖审计：`cargo deny` 禁 GPL/高危，`cargo audit` 无 high | CI 步骤 |
| S10 | LLM 出站域名白名单 = `llm.baseURL` 的 host | 单测改 host 应被拒 |
| S11 | 一键清除：删数据文件 + 删密钥库条目 + 清空内存罐 | 手工验收 |

### 7.3 限流与礼貌抓取

- 同 host 最小间隔 300 ms；全局并发 ≤ 2；单源单轮页数上限沿用 `pages`。
- 响应体上限 6 MiB；重定向上限 4（列表）/ 8（登录）；超限即失败而非截断。
- 遵守 `Retry-After`；对 429/503 退避 30 s 起，最多 3 次。

---

## 8. 双端差异矩阵

| 能力 | Windows | Android | 处理方式 |
| --- | --- | --- | --- |
| 定时抓取 | 进程常驻 + `tokio::interval` | WorkManager 周期任务（最小 15 min，非精确） | `PollScheduler` trait 两套实现 |
| 到点提醒 | 15 s 心跳 | 前台服务运行时 15 s；否则降级为"抓取后 + 到期日 09:00" | `ReminderDriver` trait |
| 系统通知 | `tauri-plugin-notification`（AppNotification） | 同插件（需 `POST_NOTIFICATIONS` 运行时权限） | 统一封装 `Notifier` |
| 开机自启 | `tauri-plugin-autostart` | `BOOT_COMPLETED` 广播 + 重排 WorkManager | 各自实现 |
| 托盘 | 支持（`TrayIcon`） | 无 | `#[cfg(desktop)]` |
| 密码存储 | Credential Manager | Android Keystore | `SecretStore` trait |
| 本地 HTTP / RSS | 保留（回环，供手机/阅读器订阅） | 取消，改"分享 / 导出" | `#[cfg(desktop)]` |
| 布局 | 侧栏 + 三栏内容 | 底部导航 + 单列 + 44 dp 触控目标 | CSS 断点 + `platform` 类名 |
| 后台被系统杀 | 不适用 | 常见 | 每次回到前台做一次增量补抓 |

Android 的三条硬约束必须在 UI 里说清，不能假装和桌面一致：① 不能保证 15 s 级提醒；② 深度 Doze 下抓取可能延后到数小时；③ 引导用户把应用加入电池优化白名单。

---

## 9. 可执行改造步骤

每个阶段都有"进门条件"和"出门门槛"，不达标不进入下一阶段。

### P0 · 基线冻结（不改一行生产代码）

1. **录制**（手工，浏览器"另存为"或 `curl --compressed -o`）30 份真实响应到 `fixtures/raw/`，文件名即用例名，形如 `html__portal-list-1.html`、`rss__sspai.xml`、`json__bjut-news.json`。覆盖面：HTML 列表 10（含 1 份 GBK）、RSS 5、JSON 5、网关改写后的 HTML 5、登录页与错误体 5。
2. **可选 meta**：`<用例名>.meta.json` 提供 `baseUrl` / `source`（`itemsPath`、`map`、`urlTemplate`、`linkPattern`）/ `keywords` / `content`（详情页正文，用于对照抽取与打分）。
3. **导出期望值**：`node tools/golden.mjs --fixtures fixtures/raw --out fixtures/expected --pretty`。脚本不联网，直接复用现有 `core.mjs`，输出每个用例的 `RawItem[]` 与逐条的 `extract / score / rule`。
4. 冻结 `PROMPT_VERSION = 1` 与当前 system prompt 文本，供后续对照。

**出门门槛**：`golden.mjs` 输出 30/30 成功；fixtures 覆盖 3 种源类型 + GBK + 登录过期 + WebVPN 还原 + 边界日期（2 月 29 日、"22 2026.10" 格式）；`bodySource=none` 的用例不得出现"摘要取到导航文字"。

> 已实现：`tools/golden.mjs`（零依赖，`node tools/golden.mjs --help`）。注意它与 `store.mjs` 的 `bodyText` 规则一致——`json` 源可用列表摘要兜底，`html/rss` 源的 `context` 是页面碎片，必须由 `meta.content` 提供正文，否则摘要会退化成导航文字。

### P1 · `cn-core` 移植

```bash
cargo new --lib crates/cn-core
cargo test -p cn-core --test golden -- --nocapture
```

移植顺序：`clean_title` → `find_date` → `find_deadline` → `find_contact` → `parse_html_list` → `parse_rss` → `parse_json_list` → `extract_main_content` → `score` → `rule_summary`。

**出门门槛**：字段级一致率 ≥ 98%；`score` 与 `deadline` 一致率 100%（二者是确定性函数，不允许有偏差）；`cargo clippy -D warnings` 干净。

### P2 · `cn-net`

```bash
cargo test -p cn-net
```

必测：`restore(rewrite(u)) == u` 的属性测试（proptest，1000 例）；GBK/UTF-8/BOM 三种解码；gzip/br/deflate；4 次重定向后放弃；6 MiB 截断；网关登录页识别 6 个样本。

**出门门槛**：WebVPN 样例 100% 通过；GBK 页面抽样 5 个零乱码；超时与重定向用例全绿。

### P3 · `cn-store`

```bash
cargo test -p cn-store
```

实现 18 个 op、原子落盘、提醒引擎。时钟用 `Clock` trait 注入，测试覆盖 `due-1min / due / due+5min / due+6min` 四个边界。

**出门门槛**：18 个 op 各有成功 + 失败用例；提醒 `fired[]` 去重与 ±窗口边界全绿；进程崩溃后 `store.json` 仍可解析（写 1000 次随机中断模拟）。

### P4 · 桌面壳

```bash
cd apps/app && cargo tauri dev
```

1. 迁移前端到 `ui/`，把 `fetch('/api/action')` 换成 `invoke('act')`，加一层 `api.ts` 适配（保留原 op 名与载荷）。
2. 四条命令先行：`get_state` / `act` / `llm_test` / `export_data`。
3. 托盘、自启动、单实例按 `#[cfg(desktop)]` 接入。

**出门门槛**：本文 §3.3 的 23 个 op 逐条点通；四页无回归；`state` 在失败响应下也被应用（断网后改设置应回滚）。

### P5 · 登录向导与响应式

1. 三模式登录 UI + 11 状态渲染 + 11 个错误码文案。
2. `auth://progress` 订阅与 500 ms 轮询兜底。
3. 断点：≥1008 侧栏、640–1008 折叠、<640 底部导航。

**出门门槛**：断网 / 密码错 / 验证码 / 会话过期 / 网关过期 5 个场景，每个都能从界面文字判断"下一步做什么"，不需要看日志。

### P6 · Android 壳

```bash
cd apps/app && cargo tauri android init && cargo tauri android dev
```

1. Kotlin 插件：`WorkManager` 周期任务 + `BOOT_COMPLETED` + Keystore 读写。
2. 通知权限申请、电池优化白名单引导页。
3. 移动端布局与触控目标（≥44 dp）。

**出门门槛**：杀进程后重开，日程与登录态仍在；通知可达；Doze 下 30 min 内至少触发一次抓取。

### P7 · 打包分发

```bash
cargo tauri build                      # NSIS + MSI
cargo tauri android build --apk --split-per-abi
```

**出门门槛**：Windows 安装包 ≤ 8 MB、portable zip 可解压即用（缺 WebView2 时给引导器而非白屏）；APK（arm64）≤ 12 MB；两者冷启动 ≤ 1 s。

### P8 · 回归与灰度

1. 双端各跑一遍 §10 验收表。
2. 保留 Node 版一个发布周期作为回退；`store.json` 双向可读（导出/导入）。

---

## 10. 验收标准

### 10.1 功能验收

| ID | 项 | 方法 | 阈值 |
| --- | --- | --- | --- |
| F1 | 解析一致性 | P1 golden test | 字段级 ≥ 98%，score/deadline 100% |
| F2 | 抓取完整性 | 4 个预置源各抓 2 页 | 条数与 Node 版差异 ≤ 2 条 |
| F3 | 去重 | 同源连抓 3 次 | 第二次起新增 0 条 |
| F4 | 详情跟进 | 25 篇正文 | 非空率 ≥ 90%，无导航/页脚残留 |
| F5 | 打分排序 | 固定 fixtures | 顺序与 Node 版完全一致 |
| F6 | 提醒窗口 | 冻结时钟 4 边界 | 全部按 §4.2 触发，无重复 |
| F7 | 自动建日程 | due < now-1h 的通知 | 不建；同 noticeId 只建一次 |
| F8 | 登录 11 态 | 状态机单测 + 手工 | 每态可达、可退出、无死锁 |
| F9 | 11 个错误码 | 构造对应用例 | 每码文案与 next_action 正确 |
| F10 | 会话保持 | 重启 / 杀进程 / 断网恢复 | 免重登；续期成功率 ≥ 95%（校内网） |
| F11 | LLM 三任务 | 30 条真实通知 | category 合法率 100%；gist ≤ 60 字；deadline 可解析 |
| F12 | LLM 降级 | 断开 baseURL | 100% 回落规则摘要，界面无空白 |
| F13 | 双端一致 | 同 fixtures 跑两端 | `StateDto` 逐字段相等 |
| F14 | 检索召回 | 60 题 golden（`docs/rag-on-device-design.md` §10） | Recall@5 ≥ 0.85，MRR ≥ 0.65 |
| F15 | 生成可核验 | 引用 `quote` 必须是块内原文子串 | 支撑率 ≥ 0.95 |
| F16 | 不可答拒答 | 15 道语料外问题 | 拒答准确率 ≥ 0.90，不得编造 |

### 10.2 非功能验收

| ID | 项 | 阈值 |
| --- | --- | --- |
| N1 | 冷启动 | Windows ≤ 1 s，Android ≤ 1.5 s |
| N2 | 安装体积 | Windows ≤ 8 MB，APK arm64 ≤ 12 MB |
| N3 | 内存 | 空闲 ≤ 80 MB（Windows），≤ 120 MB（Android） |
| N4 | 单轮抓取耗时 | 4 源 × 2 页 ≤ 20 s（校内网，不含 LLM） |
| N5 | 日志安全 | 全量日志 grep 无 Cookie 值 / Key / 密码 |
| N6 | 崩溃恢复 | 强杀 20 次，`store.json` 均可解析且不丢已确认的写操作 |
| N7 | 依赖合规 | `cargo deny` 无 GPL、`cargo audit` 无 high |

### 10.3 行为保留清单（不得回归）

| # | 行为 | 现状位置 |
| --- | --- | --- |
| B1 | 单源失败不影响其他源；已抓到的仍入库 | `store.mjs:495-506` |
| B2 | 登录失败只重跑失败源，最多 1 轮重试 | `:453-515` |
| B3 | 详情按关键词命中排序，限 10–40 篇 | `:517-538` |
| B4 | `maxItemsPerSource` 截断（1–200） | `:460` |
| B5 | 去重键 = 源 id + 标题 sha1 前 16 位 | `:463` |
| B6 | 提醒窗口 `[due-level, due+5min]`，`fired[]` 去重 | `:638-662` |
| B7 | 自动建日程跳过 1 小时前已过期的截止 | `:572-591` |
| B8 | 网关 Cookie 与门户 Cookie 分离发送 | `core.mjs:683-686` |
| B9 | 网关 Cookie 永不回传前端 | `store.mjs:1029-1033` |
| B10 | 响应上限 6 MiB、重定向 4/8 次 | `core.mjs:721,709` |
| B11 | 无 AI 时必须仍有摘要与"对我有什么用" | `core.mjs:401-435` |
| B12 | `state` 截断 200/300/20，存储上限 400/600/40 | `store.mjs:1059-1063,109-112` |

---

## 11. 风险与缓解

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| Rust 正则不支持断言导致解析偏差 | 日期/截止漏抽 | P1 用 golden test 卡住；`fancy-regex` 限定在单模块 |
| WebView2 缺失 | 便携版白屏 | 启动检测 + Evergreen 引导器；失败给明确文案 |
| Android 后台限制 | 提醒不准时 | 前台服务可选；UI 明示降级；引导电池白名单 |
| Android Keystore 与桌面 DPAPI 语义不同 | 密钥迁移失败 | 只迁移数据不迁移凭据，首次启动重新登录 |
| LLM 输出漂移 | 分类/字段不稳 | Schema 校验 + 缓存 `PROMPT_VERSION` + 降级规则 |
| 体积超标 | 违背 G6 | `lto=true`、`opt-level=z`、`strip=true`、按 ABI 分包 |
| 双端 UI 分叉 | 维护成本翻倍 | 单一前端产物 + 平台类名，不做两套页面 |
| 一次迁移过大 | 长时间不可用 | 按 P0–P8 分阶段，Node 版保留一个发布周期 |

---

## 12. 交付物与下一步

| 交付物 | 路径 |
| --- | --- |
| 本方案 | `docs/rust-tauri-refactor-plan.md` |
| 录制与对照脚本 | `tools/golden.mjs`（已实现，零依赖） |
| Rust 核心 | `crates/cn-core` `cn-net` `cn-store` `cn-llm` `cn-model` |
| 双端外壳 | `apps/app`（Tauri 2，含 `gen/android`） |
| 契约冻结表 | 本文 §3.3 / §10.3 |

**建议的第一刀**：P0 + P1。理由：这两步不改任何现有行为、可完全离线执行、且一旦通过就锁定了整个迁移中唯一不可逆的部分（解析与打分的等价性）。P2 之后都是可替换的外壳工程。
