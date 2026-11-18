# P0 + P1 实施记录：fixtures 基线与 cn-model / cn-core 移植

> 执行：软件工程师（寇豆码）· 2026-11-18
> 基线：`core.mjs`（1031 行，未改动）+ `tools/golden.mjs`（未改动）
> 本文记录用例清单、golden 一致率、差异清单与决策记录。

---

## 1. P0 · fixtures + golden 基线

### 1.1 用例清单（`fixtures/raw/`，共 32 份，全部导出成功）

| 类别 | 数量 | 文件 |
| --- | --- | --- |
| HTML 列表 | 12 | `html__portal-notice` `html__jwc-teaching` `html__lecture-hall` `html__graduate-admin` `html__scholarship-list` `html__exam-arrange` `html__career-talk` `html__campus-logistics` `html__gbk-notice`（GBK 字节）`html__intl-exchange` `html__leap-day`（2028-02-29）`html__odd-date`（"22 2026.10" 格式） |
| 网关改写 HTML | 5 | `html__webvpn-portal` `html__webvpn-jwc` `html__webvpn-lecture` `html__webvpn-relative`（相对 /https/<hex>/ 路径）`html__webvpn-mixed`（网关绝对 + 相对 + 直连混合） |
| 登录页 / 错误体 | 5 | `html__cas-login`（CAS 带 errorDiv）`html__gateway-login`（网关登录页）`html__please-login`（"请登录/登录超时"）`json__err-10041` `json__err-10042` |
| RSS / Atom | 5 | `rss__campus-news` `rss__academic-report` `rss__jwc-feed` `rss__career-service` `rss__atom-library`（Atom entry/link@href） |
| JSON 列表 | 5 | `json__portal-news`（d.data + map + urlTemplate/{id}）`json__with-urls`（显式 url 字段）`json__rows-nested`（data.rows）`json__no-items-path`（guessList 兜底）`json__template-index`（urlTemplate/{index}） |

出门门槛覆盖核对：

- HTML 列表 ≥ 10 且含 1 份 GBK：**12 份，GBK 由 Python `encode('gbk')` 写真实字节**（`html__gbk-notice.html`，873 字节，`charset=gb2312`）。
- RSS 5、JSON 5、网关改写 HTML 5：**全部满足**。网关地址用 `core.mjs::encodeWengineHost` 同算法（AES-128-CFB，key=IV=`wrdvpnisthebest!`）离线算好 hex 后嵌入，形如 `https://webvpn.bjut.edu.cn/https/77726476706e69737468656265737421e7e056…/<path>`。
- 登录页与错误体 5：**满足**（CAS errorDiv、"请登录"、e=10041/10042 JSON、网关登录页）。
- 边界日期：**2028-02-29**（闰日截止，`html__leap-day.meta.content`）与 **"22 2026.10"**（findDate 第二分支，`html__odd-date` 四条标题）均命中。
- html/rss 源 meta.content 均提供详情页正文；json 源不提供（列表摘要兜底，`bodySource=context`）。

### 1.2 golden 导出

```
node tools/golden.mjs --fixtures fixtures/raw --out fixtures/expected --pretty
→ 导出 32/32 个用例，106 条条目
```

- 5 个用例解析出 0 条（5 个登录/错误体），属预期：错误体没有通知条目，且 `bodySource=none` 的 0 条用例不存在"摘要取到导航文字"问题（无条目即无摘要）。
- `generatedAt` 每用例记录导出时刻（2026-11-18T02:40Z ≈ 北京时间 10:40），Rust golden test 用它构造 `FixedClock`，使 `daysUntil`/无年份日期完全可复现。

### 1.3 与 core.mjs 的观察（本轮不改，仅记录）

- `parseHtmlList` 用 ±180 字符上下文找日期，列表页相邻条目的日期会"渗"进导航链接（如"通知公告"拿到 2026-10-22）——现状基线行为，golden 如实冻结。
- `isoDate` 只校验 1–31，不校验月份实际天数（2026-02-29 也合法）——与 JS 一致，未"修正"。
- `ruleSummary` 的 `flatBody.replace(/\s+/g,'')` 分支在标题嵌入正文时会把正文空白全部剥掉——照抄。

---

## 2. P1 · cn-model + cn-core

### 2.1 交付物（相对 ROOT）

```
crates/cn-model/Cargo.toml            crates/cn-core/Cargo.toml
crates/cn-model/src/lib.rs            crates/cn-core/src/lib.rs
crates/cn-model/src/clock.rs          crates/cn-core/src/constants.rs
crates/cn-model/src/dto.rs            crates/cn-core/src/text.rs
crates/cn-model/src/error.rs          crates/cn-core/src/date.rs
crates/cn-model/src/state.rs          crates/cn-core/src/contact.rs
                                      crates/cn-core/src/tags.rs
                                      crates/cn-core/src/normalize.rs
                                      crates/cn-core/src/content.rs
                                      crates/cn-core/src/score.rs
                                      crates/cn-core/src/summary.rs
                                      crates/cn-core/src/parse_html.rs
                                      crates/cn-core/src/parse_rss.rs
                                      crates/cn-core/src/parse_json.rs
                                      crates/cn-core/src/regex_compat.rs
                                      crates/cn-core/tests/golden.rs
```

占位 crate（保证 workspace 可编译，后续阶段填充）：`crates/cn-net` `crates/cn-llm` `crates/cn-store` `apps/app/src-tauri`（空 lib.rs + 最小 Cargo.toml）。未改 `cn-rag`。

### 2.2 golden 一致率（`cargo test -p cn-core --test golden -- --nocapture`）

| 维度 | 一致率 | 门槛 |
| --- | --- | --- |
| 用例条数 | 32/32 | — |
| title | 106/106 = 100% | ≥ 98% |
| url | 106/106 = 100% | ≥ 98% |
| itemId | 106/106 = 100% | ≥ 98% |
| publishedAt | 106/106 = 100% | ≥ 98% |
| **score** | **106/106 = 100%** | **100%（硬门槛）** |
| **deadline** | **106/106 = 100%** | **100%（硬门槛）** |
| rule（summary/useful） | 106/106 = 100% | 参考 |

**差异清单：无。** 无需白名单容忍。

单元测试（同文件另 5 项）：`normalize_due` 边界（纯日期 / 带 T / 带空格 / 不可解析）、`find_date` 闰日 2028-02-29 与 "22 2026.10" 与越界月、`clean_title` 四类前缀、`strip_red_header`（<16 连字符不剥 / ≥16 剥除 / 无红头原样）、`score` 夹紧（下限 5、上限 98、基线 22）。

### 2.3 出门门槛核对

1. ✅ `node tools/golden.mjs …` 32/32 导出成功
2. ✅ `cargo +stable-x86_64-pc-windows-gnu test -p cn-core -p cn-model` 全绿（6 测，golden 达标）
3. ✅ `cargo +stable-x86_64-pc-windows-gnu clippy -p cn-core -p cn-model --all-targets -- -D warnings` 干净
4. ✅ `cargo tree -p cn-core` 不含 reqwest / tokio / tauri（依赖：chrono / cn-model / fancy-regex / regex / serde_json / url + dev：encoding_rs）
5. ✅ `maxItemsPerSource` 默认统一 100：`cn_model::state::MAX_ITEMS_PER_SOURCE_DEFAULT = 100`，`SettingsDto::default()` 与 serde default 均取 100（`number_or` 夹紧 1–200）
6. ✅ 本文档

### 2.4 决策记录

| # | 决策 | 理由 |
| --- | --- | --- |
| D1 | **parse_* 三个函数增加 `now: &dyn Clock` 参数**（§3.1 草图未列） | `findDate` 的「M月D日」分支需要当前年；`cn-core` 禁止读系统时钟，时间必须显式注入。草图与"时钟注入"约束自相矛盾，取后者。 |
| D2 | `find_date` / `find_deadline` 的 `(?![\d.])` 等断言走 **fancy-regex**，封装在 `regex_compat` 模块 | 精确复刻 JS 回溯语义（如 "2026.10月5" 中 `月?` 回溯）；手动改写等价性论证成本高。全 crate 只有 `regex_compat` 引用 fancy-regex。 |
| D3 | JS `\d` 一律改写为 `[0-9]` | Rust `regex` 的 `\d` 是 Unicode Nd（会吃全角数字），JS 是 ASCII。 |
| D4 | JS `slice/indexOf` 的 UTF-16 下标 → Rust **字符**下标（`char_window`/`char_slice`） | 中文 1 UTF-16 单元 = 1 字符但 3 UTF-8 字节；按字节切窗口会缩成 1/3，导致 context 与日期窗口不一致。fixtures 全 BMP，字符下标与 JS 完全一致。 |
| D5 | golden test 用 `generatedAt` 构造 `FixedClock` | `daysUntil` 决定 score 加档与"还有 N 天"文案；冻结时刻后 100% 可复现，与测试运行日期无关。 |
| D6 | golden test 用 `encoding_rs::UTF_8`（WHATWG）读 fixture 原文 | Node `readFile(utf8)` 即 WHATWG 解码；GBK 用例的乱码两侧必须逐字节同构，`from_utf8_lossy` 的替换字符分组不保证一致。实测 GBK 用例 4 条标题全对。 |
| D7 | `RuleSummary` 增加 `why: Vec<String>` 字段 | 规格要求 summary/useful/why 三字段，但 `core.mjs::ruleSummary` 只返回前两者；`why` 取 `useful` 同内容（"对我有什么用"的理由），golden 只对照 summary/useful。 |
| D8 | `FieldMap` 增加 `link: Option<String>` | `parseJsonList` 的 `map.url || map.link || 'url'` 兜底链在 JS 里真实存在；保留以照搬语义。 |
| D9 | `normalize_due` 返回 `Option<String>`（"YYYY-MM-DDTHH:mm"） | 与 JS 空串 ↔ `None` 等价；直接是调用方（store/日程）要的字面格式。第三分支覆盖常见 JS `Date` 可解析格式子集（RFC2822/RFC3339/常见数值格式），非穷举——fixtures 与单测均不依赖冷门格式。 |
| D10 | **不实现 `SourceKind::Wechat`** | `store.mjs:169` 的 `fetchWechatSource` 未定义、入口不可达（已知缺陷清单）；Rust 侧不复制死分支。 |
| D11 | 占位 crate 填充 workspace 缺失成员 | 比临时改 members 更省后续工作：P2/P3 直接往里写实现，workspace 结构不再动。 |
| D12 | `ScoreInput.keywords` 保留但不参与打分 | `scoreNotice` 形参含 `keywords` 但函数体未使用，照抄签名以免后续误删语义。 |

### 2.5 已知风险与后续注意

- `days_until` 用毫秒差 / 86400000.0 的 f64 除法（与 JS 一致）；`Math.ceil` 边界若恰好落在整日毫秒差 ±100ms（golden 导出与 enrich 的毫秒级时差）可能翻 1 天——概率 ~1e-6，本次 106 条全对；若未来偶发，优先在测试侧加 ±1 天容差而不是改实现。
- `findContact` 的中文名捕获 `[\u4e00-\u9fa5]{2,4}(?:老师|同学)?` 与 JS 同样贪心（"李老师"整体命中），行为怪但一致。
- `normalize_due` 第三分支对 `new Date` 冷门格式（如 "Dec 1, 2026"）不保证覆盖；P3 接 store 时如有真实格式再补分支。
- `extract_main_content` 已移植（CMS 容器标记 + 尾部截断），golden 未覆盖（golden.mjs 用 meta.content 直传正文），P3 详情跟进阶段接入后再补真实样例。
