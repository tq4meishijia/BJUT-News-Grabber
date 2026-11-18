# 校园通知 · 日程

一个跑在自己电脑上的工具：**自动抓学校通知 → 挑出有用的部分 → 生成日程 → 到点提醒**。
不依赖云端账号，数据、凭据都留在本机。

当前实现为 **Rust 核心 + Tauri 2 桌面壳**（Windows 优先，Android 已在架构中预留）；旧的 Node / WinUI 3 实现已从仓库移除，历史提交保留在 `backup/pre-rust-reset` 标签中。

## 目录结构

```
Cargo.toml / Cargo.lock          Rust workspace（cn-* crates + cn-app）
crates/cn-model                  共享 DTO、错误码、Clock、登录状态机
crates/cn-core                   纯逻辑：解析 / 抽取 / 打分 / 规则摘要 / 日志脱敏
crates/cn-net                    HTTP、编码嗅探、Cookie、WebVPN、CAS 登录、SSRF 校验
crates/cn-llm                    OpenAI 兼容客户端、Schema 校验、缓存、月度预算
crates/cn-store                  持久化、23 个 op、抓取流水线、提醒引擎
crates/cn-rag                    端侧检索内核（切块 / BM25 / 余弦 / RRF）
apps/app/src-tauri               Tauri 2 桌面壳：命令层 + 托盘 + 系统通知
apps/app/ui                      前端（单份产物，双端共用）
fixtures/ raw + expected         录制响应与 golden 基线（32 用例）
tools/golden.mjs                 Node 基线导出脚本（不联网）
docs/                            方案、实施记录、交接
.github/workflows/               Windows 自动构建（Actions）
```

## 构建与运行

开发机（本机无 MSVC 链接器时使用 GNU 工具链）：

```bash
cargo +stable-x86_64-pc-windows-gnu test --workspace
cargo +stable-x86_64-pc-windows-gnu build -p cn-app
```

有 Visual Studio Build Tools（C++ 工作负载）与 tauri-cli 时可直接跑应用：

```bash
cd apps/app
cargo tauri dev
```

`.cargo/config.toml` 含 windows-gnu 的 cdylib 链接修复，**请勿删除**。
CI 使用 `--exclude-libs,ALL` 之外的方式（MSVC 工具链）构建，见 `.github/workflows/build-windows.yml`。

## 功能概览

- **通知**：HTML / RSS / JSON 三类源；抓取、去重、详情跟进、关键词排序、打分。
- **摘要与分类**：AI 批量摘要 / 分类 / 结构化抽取，Schema 校验失败自动降级为规则摘要；可设置月度 token 预算；敏感源可单独关闭「正文送 AI」。
- **日程与提醒**：识别截止时间自动建日程；15 秒心跳判定提醒窗口，支持系统通知。
- **登录**：窗口登录 / 账号密码 / 粘贴 Cookie 三种模式；11 态状态机，会话自动保持与过期续期。
- **校外网络**：BJUT WebVPN 地址改写已内置，设置页开关即可启用。
- **隐私**：Cookie 与 API Key 不落明文、不回传前端；日志按 S3 规则脱敏；导出数据不含登录态。

## 数据与配置

- `%APPDATA%\campus-notice\store.json`：通知、日程、数据源定义（不含 Cookie）。
- `%APPDATA%\campus-notice\config.json`：端口、LLM 配置与月度预算。
- 凭据优先存 Windows Credential Manager；不可用时落加密文件 `sessions.json.enc`。

## 文档

- 方案：`docs/rust-tauri-refactor-plan.md`
- 实施记录：`docs/impl-notes-p0-p1.md`、`docs/impl-notes-p2-p3.md`、`docs/impl-notes-p4-p5.md`
- 交接与待办：`docs/handoff.md`
- 端侧 RAG：`docs/rag-on-device-design.md`

## 许可

MIT — 见 `LICENSE`。
