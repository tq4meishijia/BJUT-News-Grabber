# 校园通知 · 日程（独立程序）

一个跑在你自己电脑上的小工具：**自动抓学校通知 → 挑出对你有用的部分 → 生成日程 → 到点提醒**。
不需要 DSH，不需要 npm 依赖，只要装过 Node.js。

---

## 北京工业大学同学：开箱就有 4 个数据源

程序已经预置好校内一站式平台的 4 个通知分类，装上即可用（内部用的是 `category_id` + `p` 翻页）：

| 数据源 | 抓取地址 | 翻页 |
| --- | --- | --- |
| 一站式 · 校发通知 | `https://my.bjut.edu.cn/site/get_news_list?category_id=1&p=1&size=32` | 2 页 |
| 一站式 · 公示公告 | `...?category_id=3&p=1&size=32` | 2 页 |
| 一站式 · 教学通告 | `...?category_id=4&p=1&size=32` | 2 页 |
| 一站式 · 院部处通知 | `...?category_id=6&p=1&size=32` | 3 页 |

**第一次用需要登录一次**，二选一：

1. **设置页 → 账号密码自动登录**（推荐）：
   - 认证入口地址：`https://cas.bjut.edu.cn`
   - 登录后返回的地址：`https://my.bjut.edu.cn/common/actionCasLogin?redirect_url=https://my.bjut.edu.cn/page/site/index`
   - 填学号 / 密码 → 「保存并登录」。登录态过期时会自动重新登录。
2. **手动 Cookie**：浏览器登录门户后按 F12 → Network → 任意请求 → 复制整条 Cookie，粘到每个数据源的「登录态 Cookie」。

不登录也能先看看界面：数据源会显示「登录态失效（接口返回请登录）」，登录后点「立即抓取」即可。

两个容易踩的坑（程序里已经配好，自己加源时注意）：

- 翻页参数是 **`p`**（不是 `page`），`size` 无效，每页固定 32 条；
- 日期字段是 **`createdate`**，不映射的话所有通知都没有日期（排序、年份筛选会失效）；
- 每轮每个源最多入库 `maxItemsPerSource` 条（默认 25），抓到的比它多时，超出的部分不会自动补进来。

---

## 怎么启动

| 方式 | 做法 | 说明 |
| --- | --- | --- |
| 带窗口（推荐先试这个） | 双击 `start.cmd` | 弹一个命令行窗口显示日志，浏览器自动打开界面；关窗口或按 Ctrl+C 就停 |
| 不想要窗口 | 双击 `start-hidden.vbs` | 后台运行，没有窗口可关；用下面的「怎么停止」 |
| 开机自动跑 | 双击 `install-autostart.cmd` | 在「启动」文件夹里放一个快捷方式；取消见下方 |

> 三个 `.cmd` 都是**纯 ASCII + CRLF**，中文提示由 `node ctl.mjs` 打印（Node 直接写控制台，不受代码页影响），所以不会再出现「中文把批处理解析搞坏」的问题。

启动后浏览器会打开 **http://127.0.0.1:8737/**。这个服务只监听本机回环地址，局域网和外网都访问不到。

常用参数：

```powershell
node server.mjs --port 9000          # 换端口
node server.mjs --no-open            # 只启动，不自动开浏览器
node server.mjs --data D:\cn.json    # 换数据文件位置
```

第一次启动会在 `%APPDATA%\campus-notice\` 下生成 `store.json`（你的通知和日程）和 `config.json`（端口、API Key）。

---

## 怎么停止（隐身启动也能停）

启动时服务会把 **PID 和端口**写进 `%APPDATA%\campus-notice\server.json`，停止就是照这份记录来的：

| 想要的效果 | 做法 |
| --- | --- |
| 停掉后台实例 | 双击 **`stop.cmd`**（优雅退出：先走 `/api/shutdown`，存盘、删记录；万一卡住才按 PID 强杀） |
| 只想再打开界面 | 双击 **`open.cmd`**，或 `node ctl.mjs open` |
| 看它在不在跑、跑多久、抓了几条 | `node ctl.mjs status` |
| 确认有实例在跑（脚本判断用） | `node ctl.mjs check`（在跑返回 0） |
| 取消开机启动 | `node ctl.mjs autostart-off`（或删掉「启动」文件夹里的 `campus-notice.lnk`） |
| 手动收尾（ctl 也没法用时） | 任务管理器 →「详细信息」→ 结束**命令行里含 `server.mjs`** 的那个 `node.exe`。**别直接结束所有 node.exe**，DSH 自己也是 node |

`stop.cmd` 的输出里会写清楚这次是正常退出、超时强杀，还是「没有在运行的实例」。

---

## 界面上有什么

**通知**：全部 / 重要 / 未读 三种筛选。每条显示重要度色点、来源、日期、识别到的截止时间、联系人、标签，以及「对我有什么用」清单。可以标已读、AI 重新摘要、一键加入日程、移除。

**日程**：按「已逾期 / 今天 / 明天 / 本周内 / 更远」分组。可以手动加（标题、时间、备注、提前几分钟提醒），也可以改时间、重置提醒、标完成、删除。

**数据源**：启用/停用、编辑、测试、删除、新增。三个预置源：

| 名称 | 说明 |
| --- | --- |
| 示例 · 学校通知列表（HTML） | 把网址换成你们学校通知公告页，启用即可 |
| 示例 · 通知接口（JSON） | 纯前端渲染的站点用这个，填 itemsPath 和字段映射 |
| 示例 · RSS / Atom | 贴 feed 地址，解析方式留 HTML 也能自动识别 |

**设置**：抓取间隔、关注关键词、AI 摘要开关、自动建日程开关、详情页跟进开关、桌面通知开关、**校外模式（WebVPN）**、API Key、运行状态。

---

## 校外网络：走学校 WebVPN

校内门户（`my.bjut.edu.cn` 等）一般只在校内网络可达。校外时打开「校外模式」：**列表、翻页、详情**全部走学校 WebVPN；关掉后与校内直连完全一致（**默认关闭**）。

### 怎么开

设置页 → **校外网络（WebVPN）**：

1. 勾「开启校外模式」。
2. 填「网关地址」：学校 WebVPN 的根地址，例如 `https://webvpn.example.edu.cn`。
3. 确认「重写模板」符合你们学校网关的地址形态（见下）。
4. 在浏览器里登录一次 WebVPN，F12 → Network → 复制整条 Cookie，粘到「网关会话 Cookie」，点「保存校外模式设置」。
5. 点「立即抓取」。数据源里的 `url` / `pageUrl`（`{n}` 页码）/ `detailUrlTemplate`（`{id}` 详情 id）**保持校内原始地址，一处都不用改**——改写在请求层统一完成，翻页仍按 `p={n}` 翻满，详情仍按原模板跟进。

> **网关 Cookie 与门户 Cookie 分开**：门户登录态仍存在各数据源的「登录态 Cookie」里（或由自动登录写回）；网关那层单独存在 `store.json` 的 `settings.webvpn.cookie`，程序只在发请求时附加它，不会写进数据源、不会打印到日志。账号密码也仍按原来的方式存在 `config.json`。

### 重写规则怎么按学校改

程序把每个**出站**地址先过一次 `rewriteUrl`，把响应文本里被网关改写过的链接再过 `restoreUrlsInText` 还原回门户原始地址，所以 `new URL` 拼相对链接、去重、详情跟进都仍按门户原始地址走。规则由两个配置项决定：

| 配置 | 说明 |
| --- | --- |
| `gateway` | 网关根地址，替换模板里的 `{gateway}` |
| `rewriteTemplate` | 出站地址模板，必须含且仅含一个 `{url}`，该占位符会被替换成 `encodeURIComponent(门户原始网址)` |

默认模板是 `{gateway}/proxy?url={url}`，例如：

```
https://my.bjut.edu.cn/site/get_news_list?category_id=1&p=1&size=32
→ https://webvpn.example.edu.cn/proxy?url=https%3A%2F%2Fmy.bjut.edu.cn%2Fsite%2Fget_news_list%3Fcategory_id%3D1%26p%3D1%26size%3D32
```

这只是一种常见、可逆的网关形态。你们学校的网关若把编码后的网址放在路径里（如 `{gateway}/https/{url}`）、用别的参数名或带固定后缀，照着把模板里的固定部分换掉即可，只要 `{url}` 出现一次。**还原规则由同一个模板自动推导，不需要配第二份规则**，因此对任意合法模板都满足 `restoreUrl(rewriteUrl(u)) === u`；模板不合法时回退到默认模板。响应文本里凡是按该模板生成的网关链接都会被还原成门户地址，通知链接照常可点。若网关在链接末尾追加了 `&token=…` 之类的附加 query 参数，还原时会连同这些参数一起去掉，不会在门户地址后留下尾巴。

### 会话过期怎么办

WebVPN 会话过期时网关往往返回 **HTTP 200 + 登录页**，而不是明确的错误，容易被当成「今天没通知」。程序按响应结构识别，命中后给出明确提示：

> WebVPN 网关会话已过期：返回的是登录页而不是通知内容。请在浏览器重新登录 WebVPN，把新的网关 Cookie 粘到「设置 → 校外模式」，再抓取一次

- **非 JSON 响应**：用「登录页特征」（一段可配置正则）检测。默认值只认 `<input type=password>`、标题里的「登录 / login / 身份认证」等**通用结构**，不写死某校页面文案；你们学校的登录页长得不一样时，把它改成能命中的正则（例如 `id="loginForm"|统一身份认证`）。
- **合法 JSON 响应**：先看有没有非空的业务数据容器（`d` / `data` / `items` / `list` / `rows` 等），有就一律当正常数据放行；没有再按登录/会话语义（`请登录` / `未登录` / `session expired` / `login required` / `unauthorized` 等，或 `code` / `e` / `status` 为 401/403/302 且带 `msg` / `message` / `m`）判定。门户自己的业务码（`e=10042` / `10041`）仍走原有的「登录态失效」提示。
- **声称 JSON 却返回登录页 HTML**（`JSON.parse` 失败）：校外模式下同样抛上面那条明确提示，不会静默变成 0 条。

检测对**所有** Content-Type 生效，不按类型屏蔽；靠上面这些结构特征避免误伤正常的 JSON 列表数据。关掉校外模式后整套检测都不启用。按提示重新登录 WebVPN、更新「网关会话 Cookie」即可。

### 关掉开关

关闭时不做任何 URL 改写、不附加网关 Cookie、不做网关登录页检测，请求路径与改动前一致；4 个预置源的条数、日期、翻页、详情正文都不变。

---

## 抓到什么、怎么判断「有用」

1. **解析列表页**：支持 HTML 列表页 / RSS / JSON 接口三种；能自动处理 GBK 编码、gzip 压缩、相对链接。
2. **跟进详情页**：优先在常见 CMS 容器（`vsb_content`、`v_news_content`、`TRS_Editor`、`article-content` 等）里取正文，去掉导航和页脚。
3. **抽关键信息**：截止时间（「截止」「请于…前」「截至…」等句式）、联系人/电话、关键词标签。
4. **打分**：关键词命中越多、截止越近、涉及成绩/学籍/报名越重要；招标采购类降权。
5. **摘要**：优先用 AI 写「一句话讲什么 + 对我有什么用」；没配 Key 或调用失败时，自动退回规则摘要（不会没有内容）。

**AI 摘要要自己配 Key**：设置页填 DeepSeek 的 API Key，或者设环境变量 `DEEPSEEK_API_KEY`。不配也能用，只是摘要走规则。模型默认 `deepseek-chat`，可以在 `config.json` 里改 `llm.baseURL` / `llm.model` 接别的 OpenAI 兼容服务。

---

## 账号密码自动登录（可选）

需要登录才能看的站点，除了手动贴 Cookie，也可以让它自己登录。设置页填四项：

| 字段 | 填什么 | 例子 |
| --- | --- | --- |
| 认证入口地址 | 登录页所在的站点 | `https://cas.example.edu.cn` |
| 登录后返回的地址 | 登录成功后要回到的页面 | `https://portal.example.edu.cn/` |
| 学号 / 账号 | 你在该系统里的用户名 | — |
| 密码 | 只在保存时传一次 | — |

- 点「保存并登录」会立刻试一次，成功后状态变成「自动登录可用」。
- 之后抓取时遇到「登录态失效」，会自动重新登录，并把新 Cookie 写回**同一门户域名下**的所有数据源。
- 密码用 **Windows DPAPI** 加密存在 `config.json`，只有当前 Windows 用户能解开；换电脑或换用户要重填。
- 遇到验证码会明确提示「自动登录不可用」，这时退回手动更新 Cookie。
- 只对「表单登录 + 隐藏字段 `execution`」这一类统一身份认证有效（CAS / Spring Security 那套）；别的门户结构不同就不适用。

## 提醒是怎么触发的

- 进程在跑就提醒：**不需要开着浏览器页面**。每 15 秒检查一次，到点前按你设的档位（默认提前 1 天、2 小时）弹 **Windows 系统通知**。
- 页面开着时，另外会在右上角弹浮层，并（授权后）发浏览器通知。
- 已提醒的档位记录在数据文件里，重启不会重复轰炸。
- Windows 通知被「专注助手」拦掉的话，界面上仍然有提醒。

---

## RSS 怎么用

### 当输入源：订阅别人的 RSS

「＋ 添加数据源」→ **解析方式选 `RSS / Atom`** → 网址贴 feed 地址 → 保存 → 点「测试」。

- 支持 RSS 2.0 和 Atom；自动取 `title` / `link` / `pubDate`（RFC822、ISO 都认）/ `description` 或 `content:encoded`。
- 抓到的条目和网页源走同一套流程：AI 摘要、打分、抽截止时间、自动建日程、到点提醒。
- RSS 源通常**不需要 Cookie**；只有源本身要登录时才填。
- 实测：少数派 `https://sspai.com/feed` 10 条、IT之家 `https://www.ithome.com/rss/` 60 条，日期都能正确解析。

### 当输出：把程序抓到的通知再吐成 RSS

服务同时提供一个订阅地址：

```
http://127.0.0.1:8737/rss.xml                # 最近 50 条
http://127.0.0.1:8737/rss.xml?limit=100      # 最多 100 条
http://127.0.0.1:8737/rss.xml?minScore=40    # 只看重要度 ≥ 40 的
```

每条包含标题、链接、发布时间，正文里是「摘要 + 有用点 + 截止时间 + 来源」。可以喂给手机阅读器、Feedly、Inoreader，或 n8n/Zapier 这类自动化。

**注意**：服务只监听 `127.0.0.1`，所以这个地址**只有本机能读**。想让手机或别的设备订阅，需要让它监听局域网地址——默认故意不开（校园网里暴露出去不安全）。

### 找不到 RSS 时

- 学校官网/新闻网很多不提供 RSS：那就按「HTML 列表页」抓，或者找它的 JSON 接口，按「JSON 接口」抓。
- 公众号等内容：可以用 RSSHub 之类的桥转成 RSS，再按上面「输入源」加进来。

---

## 自测（改代码后想确认没坏）

```powershell
node test-api.mjs 8737    # 对正在跑的服务跑一遍真实接口：抓取、测试源、加日程、提醒、设置、跨站拦截、静态资源
node test-toast.mjs       # 单独验证 Windows 系统通知能不能发出去
```

---

## 目录结构

```
campus-notice-app/
├─ server.mjs            本地 HTTP 服务 + 路由 + 启动流程（写 server.json 记录 PID/端口）
├─ store.mjs             存储、抓取流水线、日程、提醒
├─ core.mjs              纯逻辑：抓取/解析/正文定位/截止时间/联系人/打分/规则摘要
├─ llm.mjs               DeepSeek（OpenAI 兼容）摘要
├─ notify.mjs            Windows 系统通知（PowerShell 调 WinRT，不装模块）
├─ ctl.mjs               状态/停止/打开界面/开机启动；.cmd 的中文提示也从这里出
├─ public/
│  ├─ index.html
│  ├─ app.js             原生 JS 前端（无框架、无构建）
│  ├─ styles.css         跟随系统浅色/深色
│  └─ icon.svg
├─ start.cmd     start-hidden.vbs     ← 启动
├─ stop.cmd      open.cmd             ← 停止 / 再打开界面
├─ install-autostart.cmd              ← 开机启动
├─ test-api.mjs / test-toast.mjs
└─ package.json
```

想改判断逻辑，基本都在 `core.mjs` 里（`findDeadline`、`findContact`、`scoreNotice`、`ruleSummary`）；
想改界面，改 `public/app.js` + `public/styles.css`，保存后刷新浏览器即可。

---

