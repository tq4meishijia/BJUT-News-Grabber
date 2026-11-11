#!/usr/bin/env node
/**
 * 校园通知 · 日程 —— 独立本地服务。
 *
 *   node server.mjs              启动并自动打开浏览器
 *   node server.mjs --no-open    只启动服务
 *   node server.mjs --port 9000  换端口
 *   node server.mjs --data D:\x\store.json
 *
 * 只监听 127.0.0.1，不对外网开放；不需要任何 npm 依赖。
 */
import http from 'node:http'
import { createReadStream } from 'node:fs'
import { mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises'
import { dirname, extname, join, normalize, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn } from 'node:child_process'
import { CampusService } from './store.mjs'
import { summarizeBatch } from './llm.mjs'
import { encryptPassword } from './login.mjs'
import { toast } from './notify.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const PUBLIC_DIR = join(HERE, 'public')

/* ------------------------------ 参数与配置 ------------------------------ */

function parseArgs(argv) {
  const args = { open: true, port: null, data: null, noToast: false }
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--no-open') args.open = false
    else if (arg === '--open') args.open = true
    /* WinUI 3 客户端托管时用 --no-toast：提醒仍写入 meta.reminders，系统 Toast 由客户端 AppNotification 单一通道发送 */
    else if (arg === '--no-toast') args.noToast = true
    else if (arg === '--port') args.port = Number(argv[++index])
    else if (arg.startsWith('--port=')) args.port = Number(arg.slice(7))
    else if (arg === '--data') args.data = argv[++index]
    else if (arg.startsWith('--data=')) args.data = arg.slice(7)
    else if (arg === '--help' || arg === '-h') args.help = true
  }
  return args
}

const args = parseArgs(process.argv.slice(2))
if (args.help) {
  console.log(`校园通知 · 日程

  node server.mjs                 启动并打开浏览器
  node server.mjs --no-open       只启动服务
  node server.mjs --port 9000     指定端口（默认 8737）
  node server.mjs --data <文件>   指定数据文件位置
`)
  process.exit(0)
}

const APP_ROOT = process.env.APPDATA
  ? join(process.env.APPDATA, 'campus-notice')
  : join(HERE, 'data')
const STORE_PATH = args.data ? resolve(args.data) : join(APP_ROOT, 'store.json')
/* 配置文件跟着数据目录走：换个目录放一份就是独立的一份，不会读到别处的设置 */
const CONFIG_PATH = join(dirname(STORE_PATH), 'config.json')
/** 运行信息：PID 与端口写在这里，stop.cmd / ctl.mjs 靠它找到并优雅停掉本进程。 */
const RUNTIME_PATH = join(APP_ROOT, 'server.json')

const DEFAULT_CONFIG = {
  port: 8737,
  openBrowser: true,
  llm: { apiKey: '', baseURL: 'https://api.deepseek.com', model: 'deepseek-chat' },
}

async function loadConfig() {
  try {
    const raw = JSON.parse(await readFile(CONFIG_PATH, 'utf8'))
    return { ...DEFAULT_CONFIG, ...raw, llm: { ...DEFAULT_CONFIG.llm, ...(raw.llm || {}) } }
  } catch {
    return { ...DEFAULT_CONFIG, llm: { ...DEFAULT_CONFIG.llm } }
  }
}

async function saveConfig(config) {
  await mkdir(dirname(CONFIG_PATH), { recursive: true })
  await writeFile(CONFIG_PATH, `${JSON.stringify(config, null, 2)}\n`, 'utf8')
}

const config = await loadConfig()
const port = args.port || Number(process.env.CAMPUS_NOTICE_PORT) || config.port || 8737

/* ------------------------------ 服务 ------------------------------ */

const service = await new CampusService({
  storePath: STORE_PATH,
  config,
  port,
  log: (...parts) => console.log('[campus-notice]', ...parts),
  onConfigUpdate: (next) => saveConfig(next),
  onNotify: (title, body) => {
    /* --no-toast：客户端托管时后端只记日志并返回 true，不调用 toast()，避免与客户端 AppNotification 双发。
       不带该参数时行为与原来完全一致（start.cmd / 现有 Web 版零影响）。 */
    if (args.noToast) {
      console.log('[campus-notice] 已交给客户端系统通知（--no-toast）：', title, body)
      return true
    }
    toast(title, body)
    return true
  },
}).init()

/* ------------------------------ HTTP ------------------------------ */

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.json': 'application/json; charset=utf-8',
}

/* ------------------------------ RSS 输出 ------------------------------ */

function escapeXml(text) {
  return String(text ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

/** 把通知列表渲染成 RSS 2.0，供手机阅读器订阅。 */
function buildNoticesRss(notices, listenPort) {
  const base = `http://127.0.0.1:${listenPort}`
  const items = notices
    .map((notice) => {
      const lines = []
      if (notice.summary) lines.push(notice.summary)
      if (notice.useful?.length) lines.push(notice.useful.map((line) => `· ${line}`).join('\n'))
      if (notice.deadline) lines.push(`截止：${String(notice.deadline).replace('T', ' ')}`)
      if (notice.sourceName) lines.push(`来源：${notice.sourceName}`)
      const date = notice.publishedAt
        ? new Date(`${notice.publishedAt}T08:00:00+08:00`)
        : new Date(notice.fetchedAt || Date.now())
      const pubDate = Number.isFinite(date.getTime()) ? date.toUTCString() : new Date().toUTCString()
      return [
        '    <item>',
        `      <title>${escapeXml(notice.title)}</title>`,
        `      <link>${escapeXml(notice.url || `${base}/`)}</link>`,
        `      <guid isPermaLink="false">${escapeXml(notice.id)}</guid>`,
        `      <pubDate>${pubDate}</pubDate>`,
        `      <description>${escapeXml(lines.join('\n'))}</description>`,
        '    </item>',
      ].join('\n')
    })
    .join('\n')
  return `<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0">
  <channel>
    <title>校园通知 · 日程（本机）</title>
    <link>${base}/</link>
    <description>本机抓取的校园通知，已按重要度与时间排好</description>
    <language>zh-cn</language>
    <lastBuildDate>${new Date().toUTCString()}</lastBuildDate>
${items}
  </channel>
</rss>
`
}

function sendJson(res, status, payload) {  const body = JSON.stringify(payload)
  res.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store',
    'content-length': Buffer.byteLength(body),
  })
  res.end(body)
}

async function readJsonBody(req, limit = 512 * 1024) {
  const chunks = []
  let size = 0
  for await (const chunk of req) {
    size += chunk.length
    if (size > limit) {
      req.destroy()
      return null
    }
    chunks.push(chunk)
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}')
  } catch {
    return null
  }
}

function sameOrigin(req) {
  const origin = req.headers.origin
  if (!origin) return true
  try {
    const url = new URL(origin)
    return (url.hostname === '127.0.0.1' || url.hostname === 'localhost') && url.port === String(port)
  } catch {
    return false
  }
}

async function serveStatic(req, res, pathname) {
  const relative = pathname === '/' ? 'index.html' : pathname.replace(/^\/+/, '')
  const target = resolve(PUBLIC_DIR, normalize(relative))
  if (!target.startsWith(PUBLIC_DIR)) {
    res.writeHead(403)
    res.end('forbidden')
    return
  }
  try {
    const info = await stat(target)
    if (!info.isFile()) throw new Error('not a file')
    res.writeHead(200, {
      'content-type': MIME[extname(target).toLowerCase()] || 'application/octet-stream',
      'cache-control': 'no-cache',
      'content-length': info.size,
    })
    createReadStream(target).pipe(res)
  } catch {
    res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' })
    res.end('404')
  }
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url || '/', `http://127.0.0.1:${port}`)
  const pathname = url.pathname

  try {
    if (pathname === '/api/state') {
      if (req.method !== 'GET') return sendJson(res, 405, { ok: false, error: '只支持 GET' })
      return sendJson(res, 200, { ok: true, state: service.state() })
    }

    if (pathname === '/api/action') {
      if (req.method !== 'POST') return sendJson(res, 405, { ok: false, error: '只支持 POST' })
      if (!sameOrigin(req)) return sendJson(res, 403, { ok: false, error: '跨站请求被拒绝' })
      const body = await readJsonBody(req)
      if (body === null) return sendJson(res, 400, { ok: false, error: '请求体不是合法 JSON' })
      const op = String(body.op || '')

      // 配置类动作（不落在 store.json 里）
      if (op === 'llm.key') {
        config.llm = { ...config.llm, apiKey: String(body.apiKey || '').trim() }
        await saveConfig(config)
        return sendJson(res, 200, { ok: true, message: config.llm.apiKey ? '已保存 API Key' : '已清空 API Key', state: service.state() })
      }
      if (op === 'llm.test') {
        const sample = service.store.notices[0] || { title: '测试通知：请于2026年11月5日前提交材料', content: '联系人：张老师' }
        const result = await summarizeBatch([sample], config)
        return sendJson(res, 200, {
          ok: result.ok,
          message: result.ok ? '模型可用，返回正常' : result.reason,
          error: result.ok ? undefined : result.reason,
          state: service.state(),
        })
      }
      if (op === 'account.save') {
        const username = String(body.username || '').trim()
        const password = String(body.password || '')
        const casBase = String(body.casBase || '').trim()
        const serviceUrl = String(body.service || '').trim()
        if (!username) {
          return sendJson(res, 200, { ok: false, error: '账号不能为空', state: service.state() })
        }
        if (!/^https?:\/\//i.test(casBase) || !/^https?:\/\//i.test(serviceUrl)) {
          return sendJson(res, 200, {
            ok: false,
            error: '「认证入口地址」和「登录后返回的地址」都要填 http(s) 开头的完整网址',
            state: service.state(),
          })
        }
        let passwordEnc = config.account?.passwordEnc || ''
        if (password.trim()) {
          passwordEnc = encryptPassword(password)
          if (!passwordEnc) {
            return sendJson(res, 200, { ok: false, error: '密码加密失败（Windows DPAPI 不可用）', state: service.state() })
          }
        }
        if (!passwordEnc) {
          return sendJson(res, 200, { ok: false, error: '第一次保存需要填写密码', state: service.state() })
        }
        config.account = { casBase, service: serviceUrl, username, passwordEnc }
        await saveConfig(config)
        service.config = config
        const login = await service.autoLogin()
        return sendJson(res, 200, {
          ok: login.ok,
          message: login.ok ? login.message || service.lastLogin?.message : undefined,
          error: login.ok ? undefined : login.reason,
          state: service.state(),
        })
      }
      if (op === 'account.test') {
        const login = await service.autoLogin()
        return sendJson(res, 200, {
          ok: login.ok,
          message: login.ok ? login.message || service.lastLogin?.message : undefined,
          error: login.ok ? undefined : login.reason,
          state: service.state(),
        })
      }
      if (op === 'account.clear') {
        delete config.account
        await saveConfig(config)
        service.lastLogin = { at: new Date().toISOString(), ok: null, message: '已清除保存的账号密码' }
        return sendJson(res, 200, { ok: true, message: '已清除保存的账号密码', state: service.state() })
      }
      if (op === 'notify.test') {
        const fired = toast('测试通知', '如果你看到这条，说明桌面提醒可用。')
        return sendJson(res, 200, { ok: true, message: fired ? '已发出系统通知' : '当前平台不支持系统通知', state: service.state() })
      }

      const result = await service.act(body)
      return sendJson(res, 200, result)
    }

    if (pathname === '/api/shutdown') {
      if (req.method !== 'POST') return sendJson(res, 405, { ok: false, error: '只支持 POST' })
      if (!sameOrigin(req)) return sendJson(res, 403, { ok: false, error: '跨站请求被拒绝' })
      sendJson(res, 200, { ok: true, message: '正在退出' })
      setTimeout(() => {
        shutdown(0).catch(() => process.exit(0))
      }, 80)
      return
    }

    /* 把抓到的通知输出成 RSS：手机上的阅读器可以直接订阅 */
    if (pathname === '/rss.xml') {
      const limit = Math.min(200, Math.max(1, Number(url.searchParams.get('limit')) || 50))
      const minScore = Math.max(0, Math.min(100, Number(url.searchParams.get('minScore')) || 0))
      const notices = service.store.notices
        .filter((notice) => (notice.score || 0) >= minScore)
        .slice(0, limit)
      const xml = buildNoticesRss(notices, port)
      res.writeHead(200, { 'content-type': 'application/rss+xml; charset=utf-8', 'cache-control': 'no-store' })
      res.end(xml)
      return
    }

    if (pathname === '/api/health') {
      return sendJson(res, 200, { ok: true, port, pid: process.pid, dataPath: STORE_PATH, notices: service.store.notices.length })
    }

    if (req.method !== 'GET') {
      res.writeHead(405)
      res.end()
      return
    }
    return serveStatic(req, res, pathname)
  } catch (error) {
    console.error('[campus-notice] 请求失败：', error)
    if (!res.headersSent) sendJson(res, 500, { ok: false, error: error?.message || String(error) })
    else res.end()
  }
})

server.on('error', (error) => {
  if (error.code === 'EADDRINUSE') {
    console.error(`端口 ${port} 已被占用。换一个：node server.mjs --port 8738`)
    process.exit(1)
  }
  console.error(error)
  process.exit(1)
})

server.listen(port, '127.0.0.1', async () => {
  const url = `http://127.0.0.1:${port}/`
  console.log(`
  校园通知 · 日程
  ────────────────────────────────
  界面地址：${url}
  数据文件：${STORE_PATH}
  配置文件：${CONFIG_PATH}
  抓取间隔：每 ${service.store.settings.pollMinutes} 分钟（可在界面里改）
  停止：双击 stop.cmd（或 node ctl.mjs stop），窗口里按 Ctrl+C
`)
  try {
    await mkdir(APP_ROOT, { recursive: true })
    await writeFile(
      RUNTIME_PATH,
      `${JSON.stringify({ pid: process.pid, port, url, dataPath: STORE_PATH, startedAt: new Date().toISOString() }, null, 2)}\n`,
      'utf8',
    )
  } catch (error) {
    console.error('[campus-notice] 写入运行信息失败：', error?.message || error)
  }
  service.startTimers()
  if (args.open && config.openBrowser !== false) {
    spawn('cmd', ['/c', 'start', '', url], { windowsHide: true, stdio: 'ignore', detached: true }).unref()
  }
})

let shuttingDown = false

async function shutdown(code = 0) {
  if (shuttingDown) return
  shuttingDown = true
  console.log('正在退出…')
  service.stop()
  try {
    await service.save()
  } catch {
    /* 存不下也要能退出 */
  }
  try {
    await rm(RUNTIME_PATH, { force: true })
  } catch {
    /* 忽略 */
  }
  server.close(() => process.exit(code))
  setTimeout(() => process.exit(code), 1500)
}

process.on('SIGINT', () => {
  shutdown(0)
})
process.on('SIGTERM', () => {
  shutdown(0)
})
