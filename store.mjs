/**
 * 服务层：一份 JSON 存储 + 抓取流水线 + 日程/提醒。
 * 只依赖 node 内置模块和 ./core.mjs、./llm.mjs，不依赖 DSH。
 */
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'
import * as core from './core.mjs'
import { summarizeBatch } from './llm.mjs'
import { NO_PORTAL_SESSION, casLogin, casCallbackCandidates, decryptPassword } from './login.mjs'

const {
  emptyStore,
  numberOr,
  sha1,
  decodeBody,
  fetchRaw,
  resolveAddress,
  parseHtmlList,
  parseRss,
  parseJsonList,
  extractMainContent,
  findDeadline,
  findContact,
  pickTags,
  scoreNotice,
  ruleSummary,
  cut,
  normalizeDue,
  countdownText,
} = core

/** 校外模式会话过期的统一提示（列表过期、声称 JSON 却回登录页都用它）。 */
const WEBVPN_EXPIRED_MESSAGE =
  'WebVPN 网关会话已过期：返回的是登录页而不是通知内容。' +
  '请在浏览器重新登录 WebVPN，把新的网关 Cookie 粘到「设置 → 校外模式」，再抓取一次'

/** 响应体看起来是不是 JSON（用来兜住「接口地址但解析方式填了 HTML」的情况）。 */
function isJsonText(text) {
  const trimmed = String(text || '').trimStart()
  if (!trimmed.startsWith('{') && !trimmed.startsWith('[')) return false
  try {
    JSON.parse(trimmed)
    return true
  } catch {
    return false
  }
}

export class CampusService {
  constructor(options = {}) {
    this.storePath = options.storePath
    this.config = options.config || {}
    this.log = options.log || ((...args) => console.log(...args))
    this.onNotify = options.onNotify || (() => false)
    /* 配置写回（例如自动纠正登录返回地址），由 server 提供 */
    this.onConfigUpdate = options.onConfigUpdate || null
    this.store = null
    this.saving = Promise.resolve()
    this.polling = false
    this.pollTimer = null
    this.reminderTimer = null
    this.lastLlm = { at: '', ok: null, message: '' }
    this.lastLogin = { at: '', ok: null, message: '' }
    this.port = options.port || 0
  }

  /* ------------------------------ 存储 ------------------------------ */

  async init() {
    this.store = await this.load()
    this.trim()
    return this
  }

  async load() {
    const base = emptyStore({})
    try {
      const raw = JSON.parse(await readFile(this.storePath, 'utf8'))
      return {
        ...base,
        ...raw,
        settings: { ...base.settings, ...(raw.settings || {}) },
        sources: Array.isArray(raw.sources) && raw.sources.length ? raw.sources : base.sources,
        notices: Array.isArray(raw.notices) ? raw.notices : [],
        items: Array.isArray(raw.items) ? raw.items : [],
        meta: { ...base.meta, ...(raw.meta || {}) },
      }
    } catch {
      return base
    }
  }

  save() {
    const snapshot = JSON.stringify(this.store)
    this.saving = this.saving.then(async () => {
      try {
        await mkdir(dirname(this.storePath), { recursive: true })
        const tmp = `${this.storePath}.tmp`
        await writeFile(tmp, snapshot, 'utf8')
        await rename(tmp, this.storePath)
      } catch (error) {
        this.log(`保存失败：${error?.message || error}`)
      }
    })
    return this.saving
  }

  trim() {
    if (this.store.notices.length > 400) this.store.notices = this.store.notices.slice(0, 400)
    if (this.store.items.length > 600) this.store.items = this.store.items.slice(0, 600)
    if (this.store.meta.reminders && this.store.meta.reminders.length > 40) {
      this.store.meta.reminders = this.store.meta.reminders.slice(0, 40)
    }
  }

  /** 校外模式配置：BJUT 专用，只认开关与网关 Cookie，其余（含网关地址）一律用内置值。 */
  webvpnConfig() {
    const raw = this.store?.settings?.webvpn
    return {
      ...core.DEFAULT_WEBVPN,
      enabled: Boolean(raw?.enabled),
      cookie: String(raw?.cookie || ''),
    }
  }

  /* ------------------------------ 抓取 ------------------------------ */

  async fetchPage(source, url) {
    const vpn = this.webvpnConfig()
    const response = await fetchRaw(url, {
      cookie: source.cookie || '',
      resolveIp: source.resolveIp || '',
      timeoutMs: 25_000,
      webvpn: vpn,
    })
    const text = decodeBody(response.body, response.headers)
    const contentType = String(response.headers['content-type'] || '')
    if (response.status === 401 || response.status === 403) {
      throw new Error(`需要登录（HTTP ${response.status}），请在「数据源」里填写登录态 Cookie`)
    }
    if (response.status >= 400) throw new Error(`HTTP ${response.status}`)
    /* 校外模式：网关会话过期常是 200 + 登录页（HTML）或 JSON 错误体，
       对所有 content-type 都先拦下来，别把登录页当成 0 条通知 */
    if (core.webvpnSessionExpired(text, vpn)) {
      throw new Error(WEBVPN_EXPIRED_MESSAGE)
    }
    /* 接口型数据源：用业务错误码判断登录态，别拿页面文案误伤正常数据 */
    if (/json/i.test(contentType)) {
      try {
        const json = JSON.parse(text)
        const message = String(json?.m || '')
        if (json && (json.e === 10042 || json.e === 10041 || /请登录|未登录|登录超时|重新登录/.test(message))) {
          throw new Error(`登录态失效（接口返回「${message || json.e}」），请更新 Cookie`)
        }
      } catch (error) {
        if (String(error?.message || '').startsWith('登录态失效')) throw error
        /* 网关声称 JSON 却返回登录页 HTML：校外模式下别把解析失败吞成 0 条 */
        if (core.isWebvpnEnabled(vpn)) throw new Error(WEBVPN_EXPIRED_MESSAGE)
      }
    } else if (/(id=["']?(?:unPassword|pwPassword|cptPassword))|cas\/login|login-normal\.html/i.test(text) && text.length < 30_000) {
      throw new Error('被重定向到登录页：登录态失效，请更新 Cookie')
    }
    /* 网关会把响应里的链接一起改写成网关地址：还原回门户原始地址，
       让 new URL / 去重 / 详情跟进仍按原来的地址走 */
    return core.restoreUrlsInText(text, vpn)
  }

  async fetchSource(source) {
    if ((source.type || 'html') === 'wechat') return this.fetchWechatSource(source)
    const pages = []
    const first = await this.fetchPage(source, source.url)
    pages.push({ html: first, url: source.url })
    const total = numberOr(source.pages, 1, 1, 5)
    if (total > 1 && source.pageUrl) {
      for (let page = 2; page <= total; page += 1) {
        const url = source.pageUrl.replace(/\{n\}/g, String(page))
        try {
          const html = await this.fetchPage(source, new URL(url, source.url).href)
          pages.push({ html, url })
        } catch {
          break
        }
      }
    }
    const all = []
    for (const page of pages) {
      const type = source.type || 'html'
      /* 地址填的是接口、但解析方式留成 HTML 时，自动按 JSON 处理（字段映射也会自动猜） */
      const sniffedJson = type === 'html' && isJsonText(page.html)
      /* 地址是 RSS/Atom 但解析方式留成 HTML 时，也自动按 RSS 处理 */
      const sniffedRss = type === 'html' && /^\s*<\?xml|^\s*<rss[\s>]|^\s*<feed[\s>]|^\s*<rdf:RDF/i.test(page.html)
      const parsed =
        type === 'rss' || sniffedRss
          ? parseRss(page.html, page.url)
          : type === 'json' || sniffedJson
            ? parseJsonList(page.html, source)
            : parseHtmlList(page.html, page.url, source)
      if (sniffedJson && !parsed.length) {
        throw new Error('这个地址返回的是 JSON，但没解析出条目：把解析方式改成「JSON 接口」，必要时填 itemsPath（如 d.data）')
      }
      if (sniffedRss && !parsed.length) {
        throw new Error('这个地址返回的是 RSS/Atom，但没解析出条目：检查 feed 是否为空，或把解析方式改成「RSS / Atom」')
      }
      for (const item of parsed) {
        if (!all.some((existing) => existing.title === item.title)) all.push(item)
      }
    }
    return { items: all }
  }

  async fetchDetailText(notice, source) {
    const vpn = this.webvpnConfig()
    /* 接口型数据源：正文往往在详情接口的某个字段里，而不是一个 HTML 页面 */
    if (source.detailUrlTemplate) {
      const url = String(source.detailUrlTemplate)
        .replace(/\{id\}/g, encodeURIComponent(notice.sourceItemId || ''))
        .replace(/\{url\}/g, encodeURIComponent(notice.url || ''))
      try {
        const response = await fetchRaw(url, {
          cookie: source.cookie || '',
          resolveIp: source.resolveIp || '',
          timeoutMs: 20_000,
          webvpn: vpn,
        })
        if (response.status >= 400) return ''
        const text = decodeBody(response.body, response.headers)
        /* 网关回登录页时别把它当成正文 */
        if (core.webvpnSessionExpired(text, vpn)) return ''
        if (source.contentPath) {
          try {
            const json = JSON.parse(text)
            const html = String(core.pickPath(json, source.contentPath) ?? '')
            const body = extractMainContent(html)
            return body.length >= 20 ? body : extractMainContent(text)
          } catch {
            return ''
          }
        }
        return extractMainContent(text)
      } catch {
        return ''
      }
    }
    try {
      const response = await fetchRaw(notice.url, {
        cookie: source.cookie || '',
        resolveIp: source.resolveIp || '',
        timeoutMs: 20_000,
        webvpn: vpn,
      })
      if (response.status >= 400) return ''
      const html = decodeBody(response.body, response.headers)
      if (core.webvpnSessionExpired(html, vpn)) return ''
      const text = extractMainContent(html)
      if (text.length >= 20) return text
      if (/<img[\s>]/i.test(html)) return '〔正文以图片或附件形式发布，建议打开原文查看〕'
      return text
    } catch {
      return ''
    }
  }

  async summarize(notices) {
    if (!this.store.settings.llm || !notices.length) return null
    const max = Math.max(1, Number(this.store.settings.llmMaxPerPoll) || 8)
    const keywords = this.store.settings.keywords || []
    /* 关键词命中多的优先用 AI 写摘要，别让配额浪费在无关通知上 */
    const ranked = notices
      .slice()
      .map((notice) => ({
        notice,
        hits: keywords.reduce((count, word) => count + (word && notice.title.includes(word) ? 1 : 0), 0),
      }))
      .sort((a, b) => b.hits - a.hits)
      .slice(0, max)
      .map((entry) => entry.notice)
    const result = await summarizeBatch(ranked, this.config)
    this.lastLlm = {
      at: new Date().toISOString(),
      ok: result.ok,
      message: result.ok ? '' : result.reason,
    }
    if (!result.ok) return null
    const byNotice = new Map()
    for (const [index, row] of result.map) {
      const notice = ranked[index]
      if (notice) byNotice.set(notice, row)
    }
    return byNotice
  }

  /**
   * 用刚拿到的 Cookie 真去请求一个同域名数据源，确认会话真的建立了。
   * CAS 登录成功不等于门户认你：返回地址填错时只会有认证服务器的 Cookie。
   */
  async probeLoginCookie(cookie, portalHost) {
    const source = this.store.sources.find((entry) => {
      try {
        return new URL(entry.url).host === portalHost
      } catch {
        return false
      }
    })
    if (!source) return true /* 没有同域名的源就没法验证，按成功处理 */
    const backup = source.cookie
    source.cookie = cookie
    try {
      await this.fetchPage(source, source.url)
      return true
    } catch (error) {
      return !/登录态失效|需要登录|请登录|WebVPN 网关会话已过期/.test(error?.message || String(error))
    } finally {
      source.cookie = backup
    }
  }
  canAutoLogin() {
    const account = this.config?.account
    return Boolean(account?.casBase && account?.service && account?.username && account?.passwordEnc)
  }

  /**
   * 用保存的账号密码登录统一身份认证，并把新 Cookie 写回同一门户下的数据源。
   * 密码用 Windows DPAPI 加密存在 config.json，只有当前 Windows 用户能解。
   */
  async autoLogin() {
    const account = this.config?.account
    const vpn = this.webvpnConfig()
    /* 走 WebVPN 时统一按门户原始地址判定，避免网关域名让 host 比较失效 */
    const effectiveService = core.restoreUrl(account?.service || '', vpn)
    if (!account?.username || !account?.passwordEnc) {
      return { ok: false, reason: '还没保存统一身份认证账号密码（设置页可以填）' }
    }
    if (!account?.casBase || !account?.service) {
      return { ok: false, reason: '还没填「认证入口地址」和「登录后返回的地址」（设置页可以填）' }
    }
    const password = decryptPassword(account.passwordEnc)
    if (!password) {
      const message = '密码解密失败：换了 Windows 用户或换了电脑，需要重新保存一次密码'
      this.lastLogin = { at: new Date().toISOString(), ok: false, message }
      return { ok: false, reason: message }
    }
    let result = await casLogin({
      casBase: account.casBase,
      service: account.service,
      username: account.username,
      password,
      webvpn: vpn,
    })
    /* CAS 说成功不代表门户认你：用新 Cookie 探测一下，不行就换认证回调地址重来 */
    if (result.ok) {
      let portalHost = ""
      try {
        portalHost = new URL(effectiveService).host
      } catch {
        portalHost = ""
      }
      const alive = portalHost ? await this.probeLoginCookie(result.cookie, portalHost) : true
      if (!alive) result = { ok: false, reason: NO_PORTAL_SESSION }
    }
    if (!result.ok && /没有下发自己的会话/.test(result.reason || "") && !/actionCasLogin|casLogin|redirect_url/i.test(account.service || "")) {
      for (const candidate of casCallbackCandidates(effectiveService)) {
        const fixed = await casLogin({
          casBase: account.casBase,
          service: candidate,
          username: account.username,
          password,
          webvpn: vpn,
        })
        if (fixed.ok) {
          let host = ""
          try {
            host = new URL(candidate).host
          } catch {
            host = ""
          }
          const ok = host ? await this.probeLoginCookie(fixed.cookie, host) : true
          if (ok) {
            result = fixed
            this.log("自动换用认证回调地址登录成功：" + candidate)
            /* 把纠正后的地址写回配置，下次不用再试错 */
            if (this.onConfigUpdate && this.config && this.config.account) {
              this.config.account.service = candidate
              try {
                await this.onConfigUpdate(this.config)
              } catch {
                /* 写失败不影响本次登录 */
              }
            }
            break
          }
          fixed.reason = NO_PORTAL_SESSION
        }
        result = fixed
        if (!/没有下发自己的会话/.test(fixed.reason || "")) break
      }
    }
    if (!result.ok) {
      this.lastLogin = { at: new Date().toISOString(), ok: false, message: result.reason }
      return { ok: false, reason: result.reason }
    }
    /* 只更新同一个门户域名下的数据源（避免把会话写到不相关的站点） */
    let portalHost = ''
    try {
      portalHost = new URL(effectiveService).host
    } catch {
      portalHost = ''
    }
    let updated = 0
    for (const source of this.store.sources) {
      let host = ''
      try {
        host = new URL(source.url).host
      } catch {
        host = ''
      }
      if (portalHost && (host === portalHost || host.endsWith(`.${portalHost}`) || host.includes(portalHost))) {
        source.cookie = result.cookie
        /* 刚换过登录态：把「登录态失效」的旧状态清掉，别让界面同时显示失效和已保存 */
        source.loginExpired = false
        if (source.status === 'error') {
          source.status = 'idle'
          source.statusText = '登录态已更新，等待下次抓取'
        }
        updated += 1
      }
    }
    await this.save()
    this.lastLogin = {
      at: new Date().toISOString(),
      ok: true,
      message: `自动登录成功，已更新 ${updated} 个数据源的登录态`,
    }
    this.log(`自动登录成功，更新了 ${updated} 个数据源`)
    return { ok: true, updated }
  }

  async poll(reason = 'timer') {
    if (this.polling) return { ok: false, message: '正在抓取中' }
    this.polling = true
    const store = this.store
    const startedAt = new Date().toISOString()
    try {
      const enabled = store.sources.filter((source) => source.enabled && source.url)
      if (!enabled.length) {
        store.meta.lastPollAt = startedAt
        store.meta.lastPollError = '没有启用任何数据源'
        await this.save()
        return { ok: false, message: '没有启用任何数据源' }
      }
      const known = new Set(store.notices.map((notice) => notice.dedupeKey))
      const fresh = []
      /* 登录态过期时自动登录重试一轮，只重跑失败的那几个源 */
      const attempts = this.canAutoLogin() ? 2 : 1
      let pending = enabled.slice()
      for (let attempt = 0; attempt < attempts && pending.length; attempt += 1) {
        const loginFailures = []
        for (const source of pending) {
          try {
            const { items } = await this.fetchSource(source)
            const sliced = items.slice(0, store.settings.maxItemsPerSource)
            let added = 0
            for (const item of sliced) {
              const dedupeKey = `${source.id}|${sha1(core.cleanTitle(item.title)).slice(0, 16)}`
              if (known.has(dedupeKey)) continue
              known.add(dedupeKey)
              fresh.push({
                id: `n-${Date.now().toString(36)}-${sha1(dedupeKey).slice(0, 8)}`,
                dedupeKey,
                sourceId: source.id,
                sourceName: source.name,
                title: item.title,
                url: item.url,
                sourceItemId: item.itemId || '',
                publishedAt: item.publishedAt || '',
                context: item.context || '',
                content: '',
                fetchedAt: startedAt,
                summary: '',
                useful: [],
                tags: [],
                deadline: '',
                contact: '',
                score: 0,
                read: false,
                relevant: true,
                ai: false,
              })
              added += 1
            }
            source.status = 'ok'
            source.statusText = `抓到 ${items.length} 条，新增 ${added} 条`
            source.lastFetchAt = startedAt
            source.lastCount = items.length
            source.loginExpired = false
          } catch (error) {
            const message = error?.message || String(error)
            source.status = 'error'
            source.statusText = message
            source.lastFetchAt = startedAt
            if (/登录态失效|需要登录/.test(message)) {
              source.loginExpired = true
              loginFailures.push(source)
            }
          }
          await new Promise((resolve) => setTimeout(resolve, 250))
        }
        if (!loginFailures.length || attempt === attempts - 1) break
        this.log(`检测到 ${loginFailures.length} 个数据源登录态失效，尝试自动登录…`)
        const login = await this.autoLogin()
        if (!login.ok) {
          this.log(`自动登录失败：${login.reason}`)
          break
        }
        pending = loginFailures
      }

      if (store.settings.fetchDetail && fresh.length) {
        /* 详情页按「标题命中关键词数」优先，别把配额浪费在无关通知上 */
        const keywords = store.settings.keywords
        const limit = Math.max(10, Math.min(40, store.settings.detailLimit || 25))
        const ranked = fresh
          .map((notice) => ({
            notice,
            hits: keywords.reduce((count, word) => count + (word && notice.title.includes(word) ? 1 : 0), 0),
          }))
          .sort((a, b) => b.hits - a.hits)
          .slice(0, limit)
        for (const entry of ranked) {
          const source = store.sources.find((item) => item.id === entry.notice.sourceId)
          /* 接口型数据源不需要 notice.url，只要配了详情接口就能取正文 */
          if (!source) continue
          if (!entry.notice.url && !source.detailUrlTemplate) continue
          if (entry.notice.url && entry.notice.url === source.url) continue
          const detail = await this.fetchDetailText(entry.notice, source)
          if (detail) entry.notice.content = core.stripRedHeader(detail)
          await new Promise((resolve) => setTimeout(resolve, 120))
        }
      }

      const aiMap = await this.summarize(fresh)
      const keywords = store.settings.keywords
      for (const notice of fresh) {
        const source = store.sources.find((item) => item.id === notice.sourceId)
        /* HTML 源的 context 是页面碎片，不能当正文；接口源的列表摘要本身就有料，可以兜底 */
        const bodyText = notice.content || (source?.type === 'json' ? notice.context : '')
        const text = `${notice.title} ${bodyText}`
        const ai = aiMap?.get(notice)
        const tags = ai?.tags?.length ? ai.tags.filter(Boolean).slice(0, 5) : pickTags(text, keywords)
        const deadline = (ai?.deadline && String(ai.deadline).trim()) || findDeadline(text)
        const contact = findContact(bodyText)
        const rule = ruleSummary({ title: notice.title, content: bodyText, tags, deadline, contact })
        notice.tags = tags
        notice.deadline = deadline
        notice.contact = contact
        notice.summary = ai?.gist ? cut(String(ai.gist), 160) : rule.summary
        notice.useful = ai?.useful?.length ? ai.useful.filter(Boolean).slice(0, 4) : rule.useful
        notice.relevant = ai ? ai.relevant !== false : true
        notice.ai = Boolean(ai)
        notice.score = scoreNotice({ title: notice.title, content: text, tags, deadline, keywords })
        notice.updatedAt = startedAt
      }

      fresh.sort((a, b) => {
        const da = a.publishedAt || a.fetchedAt
        const db = b.publishedAt || b.fetchedAt
        if (da !== db) return db.localeCompare(da)
        return b.score - a.score
      })
      store.notices = [...fresh, ...store.notices]
      this.trim()

      if (store.settings.autoSchedule) {
        for (const notice of fresh) {
          if (!notice.deadline || notice.relevant === false) continue
          /* 第一次抓取会把历史通知一起拉进来，别让过期截止时间塞满日程 */
          const due = new Date(notice.deadline).getTime()
          if (!Number.isFinite(due) || due < Date.now() - 3600_000) continue
          if (store.items.some((item) => item.noticeId === notice.id)) continue
          store.items.unshift(
            this.makeItem({
              title: cut(notice.title, 60),
              note: `来自 ${notice.sourceName}：${notice.summary}`,
              dueAt: notice.deadline,
              remindBefore: store.settings.autoRemindBefore,
              source: 'notice',
              noticeId: notice.id,
              url: notice.url,
            }),
          )
        }
      }

      store.meta.lastPollAt = startedAt
      store.meta.pollCount = (store.meta.pollCount || 0) + 1
      const failures = store.sources.filter((source) => source.enabled && source.status === 'error')
      store.meta.lastPollError = failures.length
        ? failures.map((source) => `${source.name}: ${source.statusText}`).join('；')
        : ''
      await this.save()
      return {
        ok: true,
        message: `新增 ${fresh.length} 条通知${failures.length ? `，${failures.length} 个数据源失败` : ''}`,
        added: fresh.length,
      }
    } catch (error) {
      store.meta.lastPollError = error?.message || String(error)
      await this.save()
      return { ok: false, message: error?.message || String(error) }
    } finally {
      this.polling = false
    }
  }

  /* ------------------------------ 日程 ------------------------------ */

  makeItem(input) {
    const remindBefore =
      Array.isArray(input.remindBefore) && input.remindBefore.length
        ? input.remindBefore.map((n) => numberOr(n, 30, 0, 100_000))
        : this.store.settings.autoRemindBefore.slice()
    return {
      id: `s-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
      title: String(input.title || '').trim().slice(0, 200),
      note: String(input.note || '').slice(0, 600),
      dueAt: normalizeDue(input.dueAt),
      remindBefore,
      fired: [],
      done: false,
      createdAt: new Date().toISOString(),
      source: input.source || 'manual',
      noticeId: input.noticeId || '',
      url: input.url || '',
    }
  }

  /* ------------------------------ 提醒 ------------------------------ */

  checkReminders() {
    const now = Date.now()
    let changed = false
    for (const item of this.store.items) {
      if (item.done || !item.dueAt) continue
      const due = new Date(normalizeDue(item.dueAt) || 'x').getTime()
      if (!Number.isFinite(due)) continue
      const levels = Array.isArray(item.remindBefore) && item.remindBefore.length ? item.remindBefore : [30]
      for (const minutes of levels) {
        if ((item.fired || []).includes(minutes)) continue
        const at = due - minutes * 60_000
        if (now < at) continue
        if (now > due + 5 * 60_000) continue
        item.fired = [...(item.fired || []), minutes]
        const when = new Date(due)
        const stamp = `${when.getMonth() + 1}月${when.getDate()}日 ${String(when.getHours()).padStart(2, '0')}:${String(when.getMinutes()).padStart(2, '0')}`
        const prefix = minutes > 0 ? `提前${core.describeMinutes(minutes)}：` : ''
        const suffix = minutes > 0 ? `（${countdownText(item.dueAt)}）` : ' 到点了'
        this.recordReminder(item, `${prefix}${stamp} 到期${suffix}`)
        changed = true
      }
    }
    if (changed) this.save()
    return changed
  }

  recordReminder(item, label) {
    const entry = {
      id: `r-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`,
      itemId: item.id,
      title: item.title,
      note: label,
      dueAt: item.dueAt,
      at: new Date().toISOString(),
    }
    this.store.meta.reminders = [entry, ...(this.store.meta.reminders || [])].slice(0, 40)
    if (this.store.settings.notify !== false) {
      this.onNotify(`日程提醒 · ${item.title}`, label)
    }
    this.log(`[提醒] ${item.title} —— ${label}`)
    return entry
  }

  /* ------------------------------ 动作 ------------------------------ */

  async act(input) {
    const store = this.store
    const op = String(input?.op || '')
    const now = new Date().toISOString()
    switch (op) {
      case 'poll': {
        const result = await this.poll('manual')
        return { ok: result.ok, message: result.message, state: this.state() }
      }
      case 'notice.read': {
        const notice = store.notices.find((entry) => entry.id === input.id)
        if (notice) notice.read = input.read !== false
        await this.save()
        return { ok: true, state: this.state() }
      }
      case 'notice.readAll': {
        for (const notice of store.notices) notice.read = true
        await this.save()
        return { ok: true, state: this.state() }
      }
      case 'notice.remove': {
        store.notices = store.notices.filter((entry) => entry.id !== input.id)
        await this.save()
        return { ok: true, state: this.state() }
      }
      case 'notice.summarize': {
        const notice = store.notices.find((entry) => entry.id === input.id)
        if (!notice) return { ok: false, error: '找不到这条通知', state: this.state() }
        const result = await summarizeBatch([notice], this.config)
        this.lastLlm = { at: new Date().toISOString(), ok: result.ok, message: result.ok ? '' : result.reason }
        if (!result.ok) return { ok: false, error: result.reason, state: this.state() }
        const row = result.map.get(0)
        if (!row) return { ok: false, error: '模型没有返回这条通知的摘要', state: this.state() }
        notice.summary = cut(String(row.gist || ''), 160) || notice.summary
        notice.useful = (row.useful || []).filter(Boolean).slice(0, 4)
        notice.tags = (row.tags || []).filter(Boolean).slice(0, 5)
        if (row.deadline) notice.deadline = String(row.deadline).trim()
        notice.relevant = row.relevant !== false
        notice.ai = true
        notice.score = scoreNotice({
          title: notice.title,
          content: `${notice.title} ${notice.content}`,
          tags: notice.tags,
          deadline: notice.deadline,
          keywords: store.settings.keywords,
        })
        await this.save()
        return { ok: true, state: this.state() }
      }
      case 'notices.rescore': {
        const keywords = store.settings.keywords
        let count = 0
        for (const notice of store.notices) {
          /* 顺手把红头文件页眉从已存正文里剥掉 */
          if (notice.content) notice.content = core.stripRedHeader(notice.content)
          if (notice.ai) continue
          const source = store.sources.find((entry) => entry.id === notice.sourceId)
          const bodyText = notice.content || (source?.type === 'json' ? notice.context : '')
          const text = `${notice.title} ${bodyText}`
          const tags = pickTags(text, keywords)
          const deadline = findDeadline(text)
          const contact = findContact(bodyText)
          const rule = ruleSummary({ title: notice.title, content: bodyText, tags, deadline, contact })
          notice.tags = tags
          notice.deadline = deadline
          notice.contact = contact
          notice.summary = rule.summary
          notice.useful = rule.useful
          notice.score = scoreNotice({ title: notice.title, content: text, tags, deadline, keywords })
          count += 1
        }
        await this.save()
        return { ok: true, message: `已按当前关键词重算 ${count} 条摘要`, state: this.state() }
      }
      case 'notices.refill': {
        const limit = numberOr(input.limit, 40, 1, 200)
        const keywords = store.settings.keywords
        const targets = store.notices.filter((notice) => (notice.content || '').length < 40)
        let filled = 0
        for (const notice of targets.slice(0, limit)) {
          const source = store.sources.find((entry) => entry.id === notice.sourceId)
          if (!source?.detailUrlTemplate) continue
          const detail = await this.fetchDetailText(notice, source)
          if (!detail) continue
          notice.content = core.stripRedHeader(detail)
          filled += 1
          const bodyText = notice.content
          const text = `${notice.title} ${bodyText}`
          const tags = pickTags(text, keywords)
          const deadline = findDeadline(text)
          const contact = findContact(bodyText)
          const rule = ruleSummary({ title: notice.title, content: bodyText, tags, deadline, contact })
          notice.tags = tags
          notice.deadline = deadline
          notice.contact = contact
          notice.summary = rule.summary
          notice.useful = rule.useful
          notice.score = scoreNotice({ title: notice.title, content: text, tags, deadline, keywords })
          /* 正文补回来才发现的截止时间，也要能自动落成日程 */
          if (store.settings.autoSchedule && notice.deadline) {
            const due = new Date(notice.deadline).getTime()
            if (Number.isFinite(due) && due > Date.now() - 3600_000 && !store.items.some((item) => item.noticeId === notice.id)) {
              store.items.unshift(
                this.makeItem({
                  title: cut(notice.title, 60),
                  note: `来自 ${notice.sourceName}：${notice.summary}`,
                  dueAt: notice.deadline,
                  remindBefore: store.settings.autoRemindBefore,
                  source: 'notice',
                  noticeId: notice.id,
                  url: notice.url,
                }),
              )
            }
          }
          await new Promise((resolve) => setTimeout(resolve, 120))
        }
        await this.save()
        const remaining = Math.max(0, targets.length - limit)
        return {
          ok: true,
          message: `补齐 ${filled} 条正文${remaining ? `，还有 ${remaining} 条可再点一次` : ''}`,
          state: this.state(),
        }
      }
      case 'item.add': {
        const item = this.makeItem({
          title: input.title,
          note: input.note,
          dueAt: input.dueAt,
          remindBefore: input.remindBefore,
          source: input.source || 'manual',
          noticeId: input.noticeId,
          url: input.url,
        })
        if (!item.title) return { ok: false, error: '标题不能为空', state: this.state() }
        if (!item.dueAt) return { ok: false, error: '时间格式不对，用 2026-11-05 14:30 这种', state: this.state() }
        store.items.unshift(item)
        this.trim()
        await this.save()
        return { ok: true, message: '已加入日程', id: item.id, state: this.state() }
      }
      case 'item.update': {
        const item = store.items.find((entry) => entry.id === input.id)
        if (!item) return { ok: false, error: '找不到这个日程', state: this.state() }
        if (typeof input.title === 'string') item.title = input.title.slice(0, 200)
        if (typeof input.note === 'string') item.note = input.note.slice(0, 600)
        if (typeof input.dueAt === 'string') {
          const due = normalizeDue(input.dueAt)
          if (due) {
            item.dueAt = due
            item.fired = []
          }
        }
        if (Array.isArray(input.remindBefore)) {
          item.remindBefore = input.remindBefore.map((n) => numberOr(n, 30, 0, 100_000))
          item.fired = []
        }
        await this.save()
        return { ok: true, state: this.state() }
      }
      case 'item.done': {
        const item = store.items.find((entry) => entry.id === input.id)
        if (!item) return { ok: false, error: '找不到这个日程', state: this.state() }
        item.done = input.done !== false
        item.doneAt = item.done ? now : ''
        await this.save()
        return { ok: true, state: this.state() }
      }
      case 'item.remove': {
        store.items = store.items.filter((entry) => entry.id !== input.id)
        await this.save()
        return { ok: true, state: this.state() }
      }
      case 'item.refire': {
        const item = store.items.find((entry) => entry.id === input.id)
        if (item) {
          item.fired = []
          await this.save()
        }
        return { ok: true, state: this.state() }
      }
      case 'source.add': {
        const url = String(input.url || '').trim()
        const type = ['html', 'rss', 'json'].includes(input.type) ? input.type : 'html'
        /* 公众号源允许直接填关键词 */
        if (!/^https?:\/\//i.test(url)) {
          return { ok: false, error: '请填写 http(s) 开头的网址', state: this.state() }
        }
        if (!url) return { ok: false, error: '请填写网址或关键词', state: this.state() }
        let fallbackName = url
        try {
          fallbackName = new URL(url).host
        } catch {
          /* 关键词当名字 */
        }
        store.sources.push({
          id: `src-${Date.now().toString(36)}`,
          name: String(input.name || fallbackName).slice(0, 60),
          type,
          url,
          enabled: input.enabled !== false,
          cookie: String(input.cookie || ''),
          resolveIp: String(input.resolveIp || '').trim(),
          linkPattern: String(input.linkPattern || '').trim(),
          itemsPath: String(input.itemsPath || '').trim(),
          pages: numberOr(input.pages, 1, 1, 5),
          pageUrl: String(input.pageUrl || '').trim(),
          urlTemplate: String(input.urlTemplate || '').trim(),
          detailUrlTemplate: String(input.detailUrlTemplate || '').trim(),
          contentPath: String(input.contentPath || '').trim(),
          map: input.map && typeof input.map === 'object' ? input.map : null,
          note: String(input.note || '').slice(0, 300),
          status: 'idle',
          statusText: '',
          lastFetchAt: '',
          lastCount: 0,
        })
        await this.save()
        return { ok: true, message: '已添加数据源', state: this.state() }
      }
      case 'source.update': {
        const source = store.sources.find((entry) => entry.id === input.id)
        if (!source) return { ok: false, error: '找不到这个数据源', state: this.state() }
        for (const key of [
          'name',
          'type',
          'url',
          'resolveIp',
          'linkPattern',
          'itemsPath',
          'note',
          'cookie',
          'pageUrl',
          'urlTemplate',
          'detailUrlTemplate',
          'contentPath',
        ]) {
          if (typeof input[key] === 'string') source[key] = input[key]
        }
        if (input.map && typeof input.map === 'object') source.map = input.map
        if (Number.isFinite(Number(input.pages))) source.pages = numberOr(input.pages, 1, 1, 5)
        if (typeof input.enabled === 'boolean') source.enabled = input.enabled
        await this.save()
        return { ok: true, state: this.state() }
      }
      case 'source.remove': {
        store.sources = store.sources.filter((entry) => entry.id !== input.id)
        await this.save()
        return { ok: true, state: this.state() }
      }
      case 'source.test': {
        const source = store.sources.find((entry) => entry.id === input.id)
        if (!source) return { ok: false, error: '找不到这个数据源', state: this.state() }
        let relogged = false
        for (let attempt = 0; attempt < 2; attempt += 1) {
          try {
            const { items } = await this.fetchSource(source)
            source.status = 'ok'
            source.statusText = relogged
              ? `已自动重新登录，解析出 ${items.length} 条`
              : `测试成功：解析出 ${items.length} 条`
            source.lastFetchAt = now
            source.lastCount = items.length
            source.loginExpired = false
            await this.save()
            return {
              ok: true,
              message: source.statusText,
              preview: items.slice(0, 5).map((item) => ({ title: item.title, url: item.url, date: item.publishedAt })),
              state: this.state(),
            }
          } catch (error) {
            const text = error?.message || String(error)
            const loginExpired = /登录态失效|需要登录/.test(text)
            source.status = 'error'
            source.statusText = text
            source.loginExpired = loginExpired
            /* 手点「测试」时也顺便自愈一次：能自动登录就重登再试 */
            if (loginExpired && !relogged && this.canAutoLogin()) {
              const login = await this.autoLogin()
              if (login.ok) {
                relogged = true
                await this.save()
                continue
              }
              source.statusText = `${text}（自动登录失败：${login.reason}）`
            }
            await this.save()
            return { ok: false, error: source.statusText, state: this.state() }
          }
        }
        return { ok: false, error: source.statusText, state: this.state() }
      }
      case 'settings.save': {
        const patch = input.settings || {}
        const settings = store.settings
        if (Number.isFinite(Number(patch.pollMinutes))) settings.pollMinutes = numberOr(patch.pollMinutes, 5, 5, 1440)
        if (Array.isArray(patch.keywords)) {
          settings.keywords = patch.keywords.map((word) => String(word).trim()).filter(Boolean).slice(0, 200)
        }
        for (const key of ['llm', 'autoSchedule', 'fetchDetail', 'notify']) {
          if (typeof patch[key] === 'boolean') settings[key] = patch[key]
        }
        if (Number.isFinite(Number(patch.llmMaxPerPoll))) settings.llmMaxPerPoll = numberOr(patch.llmMaxPerPoll, 8, 1, 40)
        if (Number.isFinite(Number(patch.detailLimit))) settings.detailLimit = numberOr(patch.detailLimit, 25, 5, 60)
        if (Number.isFinite(Number(patch.maxItemsPerSource))) {
          settings.maxItemsPerSource = numberOr(patch.maxItemsPerSource, 25, 1, 200)
        }
        if (Array.isArray(patch.autoRemindBefore) && patch.autoRemindBefore.length) {
          settings.autoRemindBefore = patch.autoRemindBefore.map((n) => numberOr(n, 60, 0, 100_000))
        }
        /* 校外模式只存开关与网关 Cookie；网关地址、重写模板、登录页特征一律用内置值 */
        if (patch.webvpn && typeof patch.webvpn === 'object') {
          const current = settings.webvpn || core.DEFAULT_WEBVPN
          const next = {
            ...core.DEFAULT_WEBVPN,
            enabled: Boolean(current.enabled),
            cookie: String(current.cookie || ''),
          }
          if (typeof patch.webvpn.enabled === 'boolean') next.enabled = patch.webvpn.enabled
          if (typeof patch.webvpn.cookie === 'string' && patch.webvpn.cookie.trim()) {
            next.cookie = patch.webvpn.cookie.trim()
          }
          if (patch.webvpn.clearCookie === true) next.cookie = ''
          settings.webvpn = next
        }
        await this.save()
        this.restartTimers()
        return { ok: true, state: this.state() }
      }
      case 'notify.test': {
        const fired = this.onNotify('测试通知', '如果你看到这条，说明桌面提醒可用。')
        return { ok: Boolean(fired), state: this.state() }
      }
      default:
        return { ok: false, error: `未知操作：${op}`, state: this.state() }
    }
  }

  /* ------------------------------ 视图 ------------------------------ */

  state() {
    const store = this.store
    const vpn = store.settings.webvpn || core.DEFAULT_WEBVPN
    /* 网关 Cookie 只在服务端用，不回传给浏览器 */
    const publicWebvpn = {
      enabled: Boolean(vpn.enabled),
      gateway: core.WEBVPN_GATEWAY,
      hasCookie: Boolean(vpn.cookie),
    }
    return {
      settings: { ...store.settings, webvpn: publicWebvpn },
      sources: store.sources.map((source) => ({
        id: source.id,
        name: source.name,
        type: source.type,
        url: source.url,
        enabled: source.enabled,
        note: source.note || '',
        hasCookie: Boolean(source.cookie),
            loginExpired: Boolean(source.loginExpired),
        resolveIp: source.resolveIp || '',
        linkPattern: source.linkPattern || '',
        itemsPath: source.itemsPath || '',
        pages: source.pages || 1,
        pageUrl: source.pageUrl || '',
        urlTemplate: source.urlTemplate || '',
        detailUrlTemplate: source.detailUrlTemplate || '',
        contentPath: source.contentPath || '',
        map: source.map || null,
        status: source.status || 'idle',
        statusText: source.statusText || '',
        lastFetchAt: source.lastFetchAt || '',
        lastCount: source.lastCount || 0,
      })),
      notices: store.notices.slice(0, 200),
      items: store.items.slice(0, 300),
      meta: {
        ...store.meta,
        reminders: (store.meta.reminders || []).slice(0, 20),
        llm: this.lastLlm,
        account: {
          username: this.config?.account?.username || '',
          casBase: this.config?.account?.casBase || '',
          service: this.config?.account?.service || '',
          configured: this.canAutoLogin(),
          lastLogin: this.lastLogin,
        },
        polling: this.polling,
        dataPath: this.storePath,
        port: this.port,
        llmKeySet: Boolean(this.config?.llm?.apiKey || process.env.DEEPSEEK_API_KEY),
      },
    }
  }

  /* ------------------------------ 定时器 ------------------------------ */

  startTimers() {
    this.restartTimers()
    setTimeout(() => {
      const enabled = this.store.sources.filter((source) => source.enabled && source.url)
      if (enabled.length && !this.store.notices.length) this.poll('boot').catch(() => {})
    }, 1500)
  }

  restartTimers() {
    if (this.pollTimer) clearInterval(this.pollTimer)
    if (this.reminderTimer) clearInterval(this.reminderTimer)
    const minutes = Math.max(5, Number(this.store.settings.pollMinutes) || 30)
    this.pollTimer = setInterval(() => {
      this.poll('timer').catch(() => {})
    }, minutes * 60_000)
    this.reminderTimer = setInterval(() => {
      try {
        this.checkReminders()
      } catch (error) {
        this.log(`提醒检查失败：${error?.message || error}`)
      }
    }, 15_000)
    // 启动时先跑一次，别等到 15 秒后（要补上停机期间错过的提醒窗口）
    this.checkReminders()
  }

  stop() {
    if (this.pollTimer) clearInterval(this.pollTimer)
    if (this.reminderTimer) clearInterval(this.reminderTimer)
    this.pollTimer = null
    this.reminderTimer = null
  }
}
