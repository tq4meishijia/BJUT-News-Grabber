# P2 + P3 实施记录：cn-net 网络认证层与 cn-store / cn-llm

> 执行：软件工程师（寇豆码）· 2026-11-18
> 基线：`core.mjs`（fetchRaw/decodeBody/WebVPN/会话判定）、`store.mjs`（18 op + 流水线 + 提醒）、
> `llm.mjs`、`login.mjs`、`server.mjs:249-315`（凭据类 op）。Node 源文件一行未动。

---

## 1. 模块图与注入方式

```
cn-model (DTO/Clock/AuthError)            ← 无 IO
   ↑
cn-core (纯逻辑，仍无 reqwest/tokio/tauri) ← 无 IO
   ↑
cn-net ──────────────┐
  │ error.rs   NetError（Timeout/TooLarge/TooManyRedirects/Network/Tls/SsrfBlocked…）
  │ fetcher.rs Fetcher trait + ReqwestFetcher（手动重定向、6 MiB 上限、DNS 覆盖、按源跳证书）
  │ mock.rs    MockFetcher（闭包/规则两种编排，记录 calls）← 测试统一入口
  │ scope.rs   ScopeId::Gateway(host)/Portal(host)（B8 两罐永不合并）
  │ cookie.rs  CookieJar + parse_set_cookie（Domain 归属）
  │ encode.rs  sniff_charset（header→BOM→meta→UTF-8）+ decode_body（encoding_rs WHATWG）
  │ webvpn.rs  Wengine AES-128-CFB 编解码 + rewrite/restore/restore_in_text + 过期判定
  │ ssrf.rs    validate_url（S6）
  │ cas.rs     jar_request（登录 8 跳手动跟随）+ cas_login（11 错误码分类）+ 回调候选
   ↑
cn-llm ──────────────┐
  │ prompt.rs  PROMPT_VERSION=1 + §6.1 三任务合并提示词
  │ schema.rs  validate_rows（§6.2：索引丢弃/长度截断/枚举归"其他"/时间置空/置信度 0.5）
  │ cache.rs   ai_hash=sha1(title+content[0..2000]+model+PROMPT_VERSION) + LlmCache
  │ client.rs  summarize_batch（缓存→调用→校验→带错重试 1 次→失败即败）
  │            summarize_with_fallback（B11：整批降级规则摘要）
   ↑
cn-store ────────────┐
  │ state.rs    StoreData/StoreSettings/SourceRecord/NoticeRecord/ItemRecord/…（store.json 形状）
  │ secrets.rs  SecretStore trait + FileSecretStore（AES-CFB sessions.json.enc 风格）+ MemSecretStore
  │ persist.rs  load_store（损坏回退默认）/ save_store（tmp+rename 原子）/ save_store_crashable
  │ pipeline.rs fetch_page/fetch_source/fetch_detail_text/enrich_notice/build_dedupe_key
  │ reminders.rs check_reminders（B6 窗口 [due-level, due+5min]）
  │ ops.rs      act()：18 store op + 5 凭据 op = 23 个
  │ service.rs  CampusService：poll 十二步编排（B1/B2/B3/B4/B5/B7/B11）+ autoLogin + state()
```

**Mock 注入方式**：所有 crate 只依赖 `cn_net::Fetcher` trait；测试把 `Arc<MockFetcher>`（闭包或
`Rule` 列表编排）塞进 `CampusService::new_for_test(store_path, fetcher, clock)`，时钟用
`cn_model::FixedClock`，凭据用 `MemSecretStore`/`FileSecretStore`。生产组合是
`ReqwestFetcher + SystemClock + FileSecretStore`。时序（源间 250ms / 同 host 300ms / 详情 120ms /
总超时 5min）走 `PipelineOptions`，测试置零。

## 2. 测试清单（全 workspace 99 绿 / 25 个测试目标）

| crate | 用例数 | 覆盖点 |
| --- | --- | --- |
| cn-model | 0（类型由 cn-core golden 全覆盖） | DTO/Clock/错误码被下游 99 例间接验证 |
| cn-core | 6 | golden 32 用例对照 + normalize_due/find_date/clean_title/strip_red_header/score 单测（P1 保持） |
| cn-net | 35 | **restore(rewrite(u))==u 属性测试 1000 例（固定种子 xorshift）**；Wengine hex 与 Node 种子双向核对 8 组；restore_urls_in_text；网关登录页识别 6 样本（含 e=10041/10042 不误判）；**GBK 5 样例零乱码**（header/meta/gb2312 三线索）+ UTF-8/UTF-16 BOM；**SSRF 12 URL**（file/ftp/data/169.254/fe80:: 拒，10/192.168/127.0.1/[::1] 放）；**重定向上限**（4 跳后 TooManyRedirects、follow=0 返回 302 供 jar_request）；**6 MiB 超限即失败**；**超时**；CAS 登录 10 例（成功/密码错→AUTH_E_CREDENTIAL/验证码→AUTH_E_CAPTCHA/无 execution→AUTH_E_FORM/无门户会话→AUTH_E_NO_PORTAL_SESSION/MFA→AUTH_E_MFA/网络→AUTH_E_NETWORK/回调候选） |
| cn-llm | 11 | 正常批量；JSON 损坏（2 次后失败）；**带错修复重试 1 次后成功**；索引越界丢弃；枚举越界归"其他"；时间非法置空（1999/自然语言）；早于发布日-1 天置空；**B11 降级**（fallback 行 summary/useful 非空、ai=false）；缓存命中不发请求；月度预算超限降级；ai_hash 稳定性 |
| cn-store | 36 | **18 op 各有成功+失败**（poll 双向、notice.read/readAll/remove/summarize、rescore、refill、item.add×2 失败/update/done/refire/remove、source.add/update/test/remove、settings.save、notify.test、llm.key/llm.test、account.save/test/clear、未知 op）；**提醒 4 边界**（due-1min/due/due+5min 触发、due+6min 不触发）+ fired 去重 + 多 level + 存储上限 40；**崩溃 1000 次随机截断**（store.json 始终可解析、旧数据不丢）+ 损坏文件回退；流水线 B1（单源失败不阻他源）/B4（截断）/B5（二次抓 0 新增）/B7（过期不建日程）/B11（每条有摘要与 useful）/json 摘要兜底；B12 视图截断 200/300/20 + 存储 400；B9 网关 Cookie/密码不入 state |
| cn-rag | 11 | 保持 P1 全绿（未动） |

门槛命令与结果：

```
cargo +stable-x86_64-pc-windows-gnu test --workspace          → 99 passed / 0 failed（25 targets 全 ok）
cargo +stable-x86_64-pc-windows-gnu clippy --workspace --all-targets -- -D warnings → 干净
cargo tree -p cn-store -p cn-llm -p cn-net | grep tauri       → 空
cargo tree -p cn-core | grep -E 'reqwest|tokio|tauri'         → 空
```

## 3. 决策记录

| # | 决策 | 理由 |
| --- | --- | --- |
| D1 | `Fetcher::fetch` 的 `follow_redirects` 语义：**0 = 不跟随**（把 302 交回给 jar_request 逐跳处理）；**N>0 = 最多 N 跳，第 N+1 个重定向 → TooManyRedirects** | jarRequest 需要逐跳收 Set-Cookie/还原 Location（JS `noFollow:true`）；而 §7.3「超限即失败」要求列表抓取 4/8 跳封顶报错。两种语义合一，均已测 |
| D2 | 6 MiB 上限按**解压后**字节数判定 | reqwest 自动解压；对 zip-bomb 更安全（JS 是压缩前字节封顶，此为有意收紧，见 §5） |
| D3 | WebVPN 往返属性测试按 `normalize(u)` 比较 | `new URL().href`/`Url::to_string()` 会补 `/`、省略默认端口；生成 URL 已规范化，restore(rewrite(normalize(u)))==normalize(u) 1000/1000 |
| D4 | CAS 门户 Cookie 归属判定**同时看 host 与 Set-Cookie Domain** | 逐行对照 `login.mjs::onSetCookie`；只看 host 会漏掉"POST 登录时下发的带 Domain 门户 Cookie" |
| D5 | `cn-llm` 直接复用 `cn_net::Fetcher` 注入（不做第二套 HTTP 抽象） | 任务允许"注入 Fetcher 或本地 stub"；少一层翻译，MockFetcher 三 crate 通用 |
| D6 | Schema 校验的"不早于发布日-1 天"在 client 侧按批次下标对齐 `published_at` 校验 | §6.2 的时间下限依赖每条通知的发布日；`validate_rows` 保持纯函数，发布日检查放编排层 |
| D7 | `summarize_batch`（失败即败，供 llm.test 报诚实结果）与 `summarize_with_fallback`（B11 整批降级）分成两个函数 | llm.test 若降级会误报"模型可用"；store 抓取路径用 fallback 版保 B11 |
| D8 | LLM 缓存/计量用 `tokio::sync::Mutex` | 需跨 await 持有；std MutexGuard 跨 await 是 clippy 硬红线 |
| D9 | 凭据存储：`SecretStore` trait + `FileSecretStore`（AES-CFB，`sessions.json.enc` + `sessions.key`） | 任务口径；OS 密钥库留 P4 换实现，trait 不变。明文 key 文件是已知过渡态，见 §5 |
| D10 | `settings.save` 的 `maxItemsPerSource` 回退统一 **100**（JS 为 25） | 任务指定的已知缺陷处理；其余 numberOr 回退值（pollMinutes 5、llmMaxPerPoll 8）保留 JS 原值 |
| D11 | `poll` 的总超时用**源间显式 deadline 检查**（超时即停、已入库不回滚） | §4.1"超时按部分成功返回"；比整任务 timeout 更可控、可测 |
| D12 | 流水线串行逐源（并发 1 ≤ 2）+ 源间 250ms、同 host 300ms | 对照 Node 实现（串行 250ms）且满足 §7.3 同 host 300ms 下限；`PipelineOptions` 可调 |
| D13 | `wechat` 源继续不实现 | store.mjs:169 不可达死分支（P1 D10 口径延续） |

## 4. 行为保留对照（B1–B12）

| # | 行为 | 落点与验证 |
| --- | --- | --- |
| B1 | 单源失败不影响其他源 | `poll_inner` 逐源 try；pipeline.rs 测试 b1 |
| B2 | 登录失败只重跑失败源，≤2 轮 | `attempts=can_auto_login()?2:1`，`pending=login_failures` |
| B3 | 详情按关键词命中排序，限 10–40 | `detail_limit.clamp(10,40)` |
| B4 | maxItemsPerSource 截断 1–200 默认 100 | state.rs 默认 + pipeline 测试 b4 |
| B5 | dedupeKey = 源 id + sha1(cleanTitle)[0..16] | `build_dedupe_key` + 测试 |
| B6 | 提醒窗口 [due-level, due+5min] + fired 去重 | reminders.rs + 4 边界测试 |
| B7 | 自动建日程跳过 due<now-1h | poll_inner 自动建日程分支 + 测试 b7 |
| B8 | 网关 Cookie 与门户 Cookie 分离发送 | FetchRequest.cookie/gateway_cookie 分字段 |
| B9 | 网关 Cookie 永不回传前端 | WebvpnDto.has_cookie；ops 测试断言 state JSON 无 cookie 值 |
| B10 | 6 MiB / 重定向 4/8 超限即失败 | ReqwestFetcher + fetch_limits 测试 |
| B11 | 无 AI 仍有摘要与"对我有什么用" | enrich_notice 规则兜底 + summarize_with_fallback + 测试 b11 |
| B12 | state 截断 200/300/20，存储 400/600/40 | state()/trim_storage + ops 测试 state_view_truncation |

## 5. 已知风险与未决项

1. **6 MiB 判定口径变化**：JS 对压缩前字节封顶，Rust 对解压后封顶（更安全、行为略紧）。若未来遇到 >6MiB 的压缩响应在 Node 能抓、Rust 报 TooLarge，属预期差异。
2. **sessions.key 明文密钥文件**：FileSecretStore 的过渡密钥存放；P4 必须换 OS 密钥库（trait 已留）。
3. **`normalize_due` 第三分支**（JS `Date` 冷门格式）仍为常见格式子集（P1 D9 遗留）。
4. **同 host 300ms 间隔按"上一个源 host"近似**（Node 是固定 250ms）；若后续要按每 host 独立节流表，改 `poll_inner` 的 gap 计算即可。
5. **详情跟进的图片兜底文案**〔正文以图片或附件形式发布…〕与 JS 字面一致，无 golden 样例（详情路径需要真站数据）。
6. `probeLoginCookie` 在 404 等非登录错误下按"会话有效"处理（与 JS 同逻辑）。

## 6. 文件清单（相对 ROOT，P2+P3 新增/覆盖）

```
crates/cn-net/{Cargo.toml,src/{lib,error,scope,fetcher,mock,cookie,encode,webvpn,ssrf,cas}.rs}
crates/cn-net/tests/{webvpn_property,encoding,ssrf,cas_login,fetch_limits}.rs
crates/cn-llm/{Cargo.toml,src/{lib,prompt,schema,cache,client}.rs}
crates/cn-llm/tests/llm.rs
crates/cn-store/{Cargo.toml,src/{lib,error,state,secrets,persist,pipeline,reminders,ops,service}.rs}
crates/cn-store/tests/{ops,reminders,crash,pipeline}.rs
crates/cn-model/src/{lib,state}.rs（P1 结构补齐：StatusDto/NoticeDto 字段/ReminderDto.at 等）
```
