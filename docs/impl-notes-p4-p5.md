# P4 + P5 实施记录：Tauri 桌面壳、登录向导与本轮修补

> 执行：软件工程师（寇豆码）· 2026-11-18（P4/P5 壳层在前序会话落盘，本轮核对、修补、补齐 §6.4 并固化构建修复）
> 基线：P2+P3 完成态（99 测全绿）；Node 源文件一行未动。

---

## 1. 交付物（相对 ROOT）

```
apps/app/src-tauri/
  src/lib.rs          # 组装：单实例/托盘/自启动/通知/opener 插件、15s 心跳 + pollMinutes 调度、state://changed 广播
  src/commands.rs     # 8 命令（§3.4）：get_state / act / auth_begin / auth_cancel / auth_import_cookie /
                      #   llm_test / export_data / import_data；失败响应必带 state（断网改设置可回滚）
  src/main.rs         # WebView2 探测 → 引导文案 / run()
  src/secrets.rs      # SecretStore 后端：Windows Credential Manager（keyring）→ FileSecretStore 加密文件兜底
  src/auth.rs         # 再导出 shim → cn_model::auth（见 §3 D1）
  src/redact.rs       # 再导出 shim → cn_core::redact（见 §3 D2）
  tauri.conf.json     # CSP= default-src 'self'；bundle nsis+msi；前端 ../ui
  capabilities/default.json
apps/app/ui/
  index.html / styles.css / app.js / api.js / icon.svg
    # api.js：fetch('/api/action') → invoke('act') 适配层（op 名与载荷与 Node 版一致）
    # app.js：四页（通知/日程/数据源/设置）+ 登录向导浮层（11 状态 + 3 步指示 + 11 错误码文案）
    # styles.css：≥1008 侧栏 / 640–1008 折叠 / <640 底部导航；pointer:coarse 触控目标 ≥44px
.cargo/config.toml    # windows-gnu 链接修复（见 §3 D3）
```

## 2. 测试与门槛（本轮全量复核）

| 命令 | 结果 |
| --- | --- |
| `cargo +stable-x86_64-pc-windows-gnu test --workspace` | **114 passed / 0 failed**（cn-model 8 + cn-core 6+6 + cn-net 35 + cn-llm 11 + cn-store 37 + cn-rag 11） |
| `cargo +stable-x86_64-pc-windows-gnu clippy --workspace --all-targets -- -D warnings` | 干净 |
| `cargo +stable-x86_64-pc-windows-gnu build -p cn-app` | 成功（exe + cdylib 均产出） |
| `cargo tree -p cn-core \| grep -E 'reqwest|tokio|tauri'` | 空（serde 为新增依赖，仅用于 `LogLine` 派生） |

新增测试：
- `cn_model::auth`：11 态可达、11 错误码文案、退避 1/5/15/60+停、小时上限+重置、8h 过期预测、单飞 10s、可取消矩阵（8 个，从壳层迁入）。
- `cn_core::redact`：cookie/authorization/api_key/password/set-cookie 脱敏、正常文本不动、LogLine 包装（6 个，从壳层迁入）。
- `cn_store::ops::send_to_llm_opt_out_keeps_content_local`：源级关闭 AI → poll 批量不送模型（ai=false、规则摘要兜底）+ 手动 summarize 被拒；对照组默认开 → ai=true（1 个）。

## 3. 决策记录

| # | 决策 | 理由 |
| --- | --- | --- |
| D1 | 登录状态机（11 态/退避/单飞/会话预测）移至 `cn_model::auth` | 纯逻辑零 tauri 依赖，符合 §2.2「平台能力经 trait 注入」；壳层测试二进制会链接 wry/WebView2 栈，在本机 GNU 工具链下无法启动（`STATUS_ENTRYPOINT_NOT_FOUND`，A/B 对照已排除 `.cargo/config.toml` 与 crate-type 影响，属工具链/环境既有问题），迁出后测试可在任何环境运行。壳层 `src/auth.rs` 留再导出 shim，引用路径不变 |
| D2 | 日志脱敏移至 `cn_core::redact`（补 `serde` 依赖） | S3 安全测试不能因 D1 的启动问题被搁置；纯字符串逻辑归 cn-core；壳层 `src/redact.rs` 留 shim |
| D3 | `.cargo/config.toml` 对 `x86_64-pc-windows-gnu` 加 `-C link-arg=-Wl,--exclude-libs,ALL` | cdylib（Android 预留）在 GNU 风格下导出整图符号（实测 129k > PE 上限 65535）导致链接失败；该 flag 禁止导出静态库成员符号后正常。MSVC/CI 不受影响；P6 若同样超限改用自定义 .def 只导出移动入口 |
| D4 | `cn-app` `[lib] test = false` | 全部单测已下沉到 cn-*；壳层测试副车启动失败会拖垮 `cargo test --workspace` |
| D5 | 退避测试对齐实现：第 3 次连续失败返回 `None` 并 `stopped` | 原断言（第 3 次仍返回 15min）与实现（≥3 停止）矛盾且从未运行过（被启动问题掩盖）；取 §5.4「连续 3 次失败置 Failed 并停自动重试，等用户操作」字面义。15min/1h 档位保留在表中供小时上限路径 |
| D6 | `sendToLlm`（§6.4）：`SourceRecord/SourceDto` 布尔字段默认 true；poll 批量与手动 summarize 均受约束 | 源级隐私开关的语义覆盖所有出站路径；已删除源的旧通知不在禁用名单（维持原行为）。UI：添加/编辑表单提供开关；首次开启 LLM 需勾选「我已知晓」（localStorage 记录），baseURL 变更确认见遗留 |
| D7 | `notice.summarize` 对关闭 AI 的源返回明确错误文案 | 手动摘要同样出正文，必须被源级开关拦截，文案指明 `sendToLlm` |
| D8 | 删除壳层死代码 `AuthTicket`（`AuthTicketDto` 为线上形状）、`AuthErrorCopy`（测试直用 `AuthErrorCode::user_message/next_action`） | privatize 后 clippy 报 dead code；两者均无生产引用 |
| D9 | §6.3 月度预算接线：`LlmKeyConfig.monthlyTokenBudget` → `resolve_llm_config` → `TokenMeter::set_budget`（AtomicU64）；`MetaDto.llmUsed/llmBudget` + 设置页展示 | 原实现 `monthly_token_budget` 恒为 0 且计量器外层 tokio Mutex 使 state() 同步读不可行；计量器本体内层 std Mutex 已是内部可变，去掉外层锁、预算可热更新 |

## 4. 已知风险与遗留

1. **壳层测试二进制在 GNU 工具链无法启动**（`STATUS_ENTRYPOINT_NOT_FOUND`）：A/B 证明与本仓库配置无关（wry/WebView2 栈的 DLL 加载问题）；`test = false` 绕开。若要恢复壳层单测，用 MSVC 工具链（需 VS Build Tools C++ 工作负载）或将测试继续下沉纯逻辑层。
2. **`baseURL` 变更二次确认**（§6.4）未实现——UI 没有 baseURL 编辑入口（在 config.json），需在引入 UI 编辑器时一并做确认。
3. **Android（P6）/ 打包（P7）/ 回归（P8）未动**：`gen/android` 不存在；`cargo tauri build` 需 tauri-cli 与 MSVC 链接器（本机无）。D3 的 cdylib 修复已为 Android 预留。
4. **`cargo tauri dev/build` 未实测**（缺 tauri-cli/MSVC）；`cargo build -p cn-app` 成功，命令层/事件层类型检查全绿。
5. 6 MiB 解压后封顶口径变化（P2 D2 遗留）与 `normalize_due` 冷门格式子集（P1 D9 遗留）不变。
