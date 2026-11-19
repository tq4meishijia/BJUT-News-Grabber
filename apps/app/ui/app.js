/* 校园通知 · 日程 —— 前端（原生 JS，无框架、无构建；Tauri withGlobalTauri 适配） */
import {
  fetchState as tauriFetchState,
  api as tauriApi,
  authBegin,
  authCancel,
  authImportCookie,
  authWebvpnLogin,
  listenTauri,
  openExternal,
  openAttachment,
  notifySystem,
} from './api.js'

const $ = (selector, root = document) => root.querySelector(selector)

const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }
const esc = (value) => String(value ?? '').replace(/[&<>"']/g, (char) => ESCAPES[char])
const pad2 = (value) => String(value).padStart(2, '0')

/* ------------------------------ 时间 ------------------------------ */

function parseDue(dueAt) {
  if (!dueAt) return null
  const text = String(dueAt).length === 10 ? `${dueAt}T23:59` : String(dueAt)
  const date = new Date(text)
  return Number.isNaN(date.getTime()) ? null : date
}

function toLocalInput(dueAt) {
  const date = parseDue(dueAt)
  if (!date) return ''
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}T${pad2(date.getHours())}:${pad2(date.getMinutes())}`
}

function fmtDue(dueAt) {
  const date = parseDue(dueAt)
  if (!date) return '未定时间'
  return `${date.getMonth() + 1}月${date.getDate()}日 ${pad2(date.getHours())}:${pad2(date.getMinutes())}`
}

function countdown(dueAt) {
  const date = parseDue(dueAt)
  if (!date) return ''
  const diff = date.getTime() - Date.now()
  const abs = Math.abs(diff)
  const days = Math.floor(abs / 86400000)
  const hours = Math.floor((abs % 86400000) / 3600000)
  const minutes = Math.floor((abs % 3600000) / 60000)
  const body = days > 0 ? `${days} 天 ${hours} 小时` : hours > 0 ? `${hours} 小时 ${minutes} 分` : `${minutes} 分钟`
  return diff >= 0 ? `还有 ${body}` : `已过 ${body}`
}

function fmtMinutes(minutes) {
  const value = Number(minutes) || 0
  if (value === 0) return '准时'
  if (value % 1440 === 0) return `${value / 1440} 天前`
  if (value % 60 === 0) return `${value / 60} 小时前`
  return `${value} 分钟前`
}

function dayLabel(dueAt) {
  const date = parseDue(dueAt)
  if (!date) return '未定时间'
  const startOf = (value) => new Date(value.getFullYear(), value.getMonth(), value.getDate()).getTime()
  const delta = Math.round((startOf(date) - startOf(new Date())) / 86400000)
  if (delta < 0) return '已逾期'
  if (delta === 0) return '今天'
  if (delta === 1) return '明天'
  if (delta <= 7) return '本周内'
  return '更远'
}

function scoreTone(score) {
  if (score >= 70) return 'var(--danger)'
  if (score >= 50) return 'var(--warn)'
  if (score >= 35) return 'var(--accent)'
  return 'var(--muted)'
}

/* --------------------------- 日期范围 / 附件 --------------------------- */

/** 本地日期 YYYY-MM-DD（与后端 clock 的 today 口径一致）。 */
function todayIso() {
  const now = new Date()
  return `${now.getFullYear()}-${pad2(now.getMonth() + 1)}-${pad2(now.getDate())}`
}

/** 今天往前推 n 天（走本地日历，避开 UTC 换算导致的跨时区差一天）。 */
function isoDaysAgo(days) {
  const now = new Date()
  const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() - Number(days))
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`
}

/** 快捷区间：近 N 天 = 今天往前 (N-1) 天 至今（含首尾共 N 天）。 */
function presetRange(days) {
  const amount = Number(days)
  if (!Number.isFinite(amount) || amount < 1) return null
  return { start: isoDaysAgo(amount - 1), end: todayIso() }
}

/**
 * 按输入框当前值判定命中的快捷项。
 * 规则：两端都空 → ''（全部不激活）；end 为空或等于今天且 start 等于 N 天前 → 'N'；其余 → 'custom'。
 */
function matchRangePreset(start, end) {
  const s = String(start || '').trim()
  const e = String(end || '').trim()
  if (!s && !e) return ''
  if (e && e !== todayIso()) return 'custom'
  for (const days of [7, 30, 90]) {
    if (s === presetRange(days).start) return String(days)
  }
  return 'custom'
}

/** 应用快捷区间：只填输入框，绝不触发抓取（避免误触）。custom 只把焦点交回开始日期。 */
function applyRangePreset(days, startEl, endEl) {
  if (days === 'custom' || days == null) {
    startEl?.focus()
    return
  }
  const range = presetRange(days)
  if (!range) return
  if (startEl) startEl.value = range.start
  if (endEl) endEl.value = range.end
}

/**
 * 刷新快捷按钮的选中态（is-active + aria-pressed）。
 * scope：'poll' 抓取浮窗（静态 DOM，需手动刷）｜'settings' 设置页（值来自 drafts）。
 */
function syncRangePresetState(scope) {
  const isPoll = scope !== 'settings'
  const root = isPoll ? $('#poll-range-dialog') : $('#view')
  if (!root) return
  const act = isPoll ? 'poll-range-preset' : 'settings-range-preset'
  const start = isPoll ? $('#poll-range-start')?.value : drafts.settings?.dateRangeStart
  const end = isPoll ? $('#poll-range-end')?.value : drafts.settings?.dateRangeEnd
  const active = matchRangePreset(start, end)
  for (const button of root.querySelectorAll(`[data-act="${act}"]`)) {
    const on = button.dataset.days === active
    button.classList.toggle('is-active', on)
    button.setAttribute('aria-pressed', on ? 'true' : 'false')
  }
}

/** 严格判断 YYYY-MM-DD，含真实日历校验（如 2024-13-40 判非法）。 */
function isIsoDay(text) {
  const value = String(text || '')
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false
  const [year, month, day] = value.split('-').map(Number)
  const date = new Date(year, month - 1, day)
  return (
    date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day
  )
}

/**
 * 与后端 validate_range 同规则、同文案的前端校验（PRD §4.1）。
 * 仅填一端合法；end > today 合法但返回 soft 轻提示。
 * @returns {{ ok: boolean, error: string, soft: string }}
 */
function validateDateRange(start, end) {
  const s = String(start || '').trim()
  const e = String(end || '').trim()
  const badFormat = '日期格式不正确，请选择有效日期（YYYY-MM-DD）'
  if (s && !isIsoDay(s)) return { ok: false, error: badFormat, soft: '' }
  if (e && !isIsoDay(e)) return { ok: false, error: badFormat, soft: '' }
  if (s && e && s > e) return { ok: false, error: '开始日期不能晚于结束日期', soft: '' }
  const today = todayIso()
  if (s && s > today) return { ok: false, error: '开始日期不能晚于今天', soft: '' }
  const soft = e && e > today ? '结束日期晚于今天，将按今天作为上限' : ''
  return { ok: true, error: '', soft }
}

/** 字节数格式化为 B / KB / MB。 */
function fmtSize(bytes) {
  const value = Number(bytes) || 0
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(1)} MB`
}

/** 附件状态 → 徽标 + 操作入口（与后端 AttachmentStatus 四态对齐）。 */
const ATTACHMENT_STATUS = {
  downloaded: { label: '已下载', cls: 'ok', act: 'attachment-open', action: '打开' },
  missing: { label: '缺失', cls: 'warn', act: 'attachment-retry', action: '重试' },
  failed: { label: '失败', cls: 'danger', act: 'attachment-retry', action: '重试' },
  tooLarge: { label: '过大', cls: 'warn', act: 'attachment-source', action: '打开原网页' },
}

/* ------------------------------ 状态 ------------------------------ */

let state = null
let tab = 'notices'
/** 上一次已渲染的标签：仅切换时播放视图进入动画，状态刷新不重复动画。 */
let lastRenderedTab = ''
let noticeFilter = 'all'
const NOTICE_PAGE_FIRST = 100
const NOTICE_PAGE_MORE = 200
/**
 * 通知列表已渲染条数：首屏 100，每次「显示更多」+200（全量可达千条，不能一次进 DOM）。
 * 只在切 tab / 换筛选 / 点「显示更多」时变；已读、展开等状态刷新不动它。
 */
let noticeLimit = NOTICE_PAGE_FIRST
let showDone = false
let openAdd = false
let openSourceAdd = false
let lastError = ''
/** 附件下载进度：null 表示未在下载；否则 { done, total, fileName }。 */
let attachmentProgress = null
const expandedNotices = new Set()

const drafts = {
  item: { title: '', dueAt: '', note: '', remind: '1440,120' },
  sourceAdd: { name: '', url: '', type: 'html', cookie: '', resolveIp: '', note: '', sendToLlm: true },
  settings: null,
  editItem: {},
  editSource: {},
}

function typing() {
  const el = document.activeElement
  return Boolean(el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName))
}

async function fetchState() {
  const data = await tauriFetchState()
  // get_state 直接返回 StateDto；统一包一层与旧 server 形状兼容
  if (data && !data.state && data.notices) state = data
  else if (data?.state) state = data.state
  return state
}

/* 全局请求计数：quiet 只抑制 toast，不抑制 pending 指示
   （慢 WebView2 上点了按钮要有即时反馈；并发用 finally 递减保证计数正确） */
let apiPending = 0
function renderApiPending() {
  let el = $('#api-pending')
  if (!el && $('#status-line')) {
    el = document.createElement('span')
    el.id = 'api-pending'
    el.className = 'api-pending'
    $('#status-line').insertAdjacentElement('afterend', el)
  }
  if (!el) return
  el.hidden = apiPending === 0
  if (apiPending > 0) el.textContent = `处理中…（${apiPending}）`
}

async function api(op, payload = {}, options = {}) {
  apiPending += 1
  renderApiPending()
  try {
    // invoke('act', { req: { op, ...payload } }) —— op 名与载荷保留 Node 版契约
    const data = await tauriApi(op, payload)
    if (data?.state) applyState(data.state)
    lastError = data?.error || ''
    if (data?.error) toast('error', '操作失败', data.error)
    else if (data?.message && !options.quiet) toast('ok', '完成', data.message)
    render(true)
    return data
  } catch (error) {
    lastError = String(error?.message || error)
    toast('error', '请求失败', lastError)
    return { ok: false, error: lastError }
  } finally {
    apiPending = Math.max(0, apiPending - 1)
    renderApiPending()
  }
}

/**
 * 让按钮在异步操作期间进入「处理中」态：禁用 + spinner + 文案替换，结束后恢复。
 * 所有耗时提交都走这里，避免慢响应下重复点击与「点了没反应」的错觉。
 * @param {HTMLButtonElement|null} button 触发按钮
 * @param {string} busyLabel 处理中显示的文案（空串则保留原文案只加 spinner）
 * @param {() => Promise<any>} task 实际操作
 */
async function withBusy(button, busyLabel, task) {
  if (!button || button.dataset.busy === '1') return undefined
  const originalText = button.textContent
  const originalDisabled = button.disabled
  button.dataset.busy = '1'
  button.classList.add('is-busy')
  button.disabled = true
  if (busyLabel) button.textContent = busyLabel
  try {
    return await task()
  } finally {
    button.dataset.busy = ''
    button.classList.remove('is-busy')
    button.disabled = originalDisabled
    button.textContent = originalText
  }
}

/* state://changed 版本号门槛：只应用更大版本 */
let appliedVersion = 0

function applyState(next, version) {
  if (!next) return
  if (typeof version === 'number') {
    if (version <= appliedVersion && appliedVersion !== 0) return
    appliedVersion = Math.max(appliedVersion, version)
  }
  state = next
}

/* ------------------------------ 提示 ------------------------------ */

const toasts = []

function pushToast(kind, title, note, ttl = 8000) {
  const id = `t-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`
  toasts.push({ id, kind, title, note })
  renderToasts()
  setTimeout(() => dismissToast(id), ttl)
}

function toast(kind, title, note, ttl) {
  pushToast(kind, title, note, ttl)
}

/** 关闭 toast：先播退出动画再移除，避免列表瞬间塌陷。 */
function dismissToast(id) {
  const el = document.querySelector(`.toast[data-toast-id="${id}"]`)
  const finish = () => {
    const index = toasts.findIndex((entry) => entry.id === id)
    if (index >= 0) toasts.splice(index, 1)
    renderToasts()
  }
  if (!el || el.classList.contains('toast-leaving')) return finish()
  el.classList.add('toast-leaving')
  setTimeout(finish, 150)
}

function renderToasts() {
  $('#toasts').innerHTML = toasts
    .map(
      (entry) => `
    <div class="toast ${entry.kind}" data-toast-id="${entry.id}">
      <div style="flex:1;min-width:0">
        <strong>${esc(entry.title)}</strong>
        ${entry.note ? `<span>${esc(entry.note)}</span>` : ''}
      </div>
      <button class="btn small ghost" data-act="toast-close" data-id="${entry.id}" aria-label="关闭">×</button>
    </div>`,
    )
    .join('')
}

/* ------------------------------ 系统提醒 ------------------------------ */

const seenReminders = new Set(JSON.parse(localStorage.getItem('campus-notice.seen') || '[]'))

function handleReminders() {
  const list = state?.meta?.reminders || []
  let changed = false
  for (const reminder of list) {
    if (seenReminders.has(reminder.id)) continue
    seenReminders.add(reminder.id)
    changed = true
    pushToast('reminder', reminder.title, reminder.note, 30000)
    // 后端 reminder://fired 已发系统通知时这里只是轻提示；直接调用做兜底（幂等靠 tag/seen 去重）
    notifySystem(`日程提醒 · ${reminder.title}`, reminder.note)
  }
  if (changed) {
    localStorage.setItem('campus-notice.seen', JSON.stringify([...seenReminders].slice(-300)))
  }
}

/* ------------------------------ 渲染 ------------------------------ */

function render(force = false) {
  if (!state) {
    renderSkeleton()
    return
  }
  if (!force && typing()) return
  const unread = state.notices.filter((notice) => !notice.read).length
  const activeItems = state.items.filter((item) => !item.done).length
  const dueSoon = state.items.filter((item) => {
    if (item.done) return false
    const due = parseDue(item.dueAt)
    return due ? due.getTime() - Date.now() <= 86400000 : false
  }).length

  const baseStatus = state.meta.lastPollAt
    ? `上次抓取 ${String(state.meta.lastPollAt).replace('T', ' ').slice(5, 16)} · 每 ${state.settings.pollMinutes} 分钟自动抓一次${
        dueSoon ? ` · ${dueSoon} 项 24 小时内到期` : ''
      }`
    : `还没抓取过 · 每 ${state.settings.pollMinutes} 分钟自动抓一次`
  // 附件下载进度优先显示在状态行（attachment://progress done 后清除）
  $('#status-line').textContent = attachmentProgress
    ? `正在下载附件 ${attachmentProgress.done}/${attachmentProgress.total}`
    : baseStatus

  $('#tabs').innerHTML = [
    { key: 'notices', label: `通知${unread ? ` ${unread}` : ''}` },
    { key: 'schedule', label: `日程${activeItems ? ` ${activeItems}` : ''}` },
    { key: 'sources', label: '数据源' },
    { key: 'settings', label: '设置' },
  ]
    .map(
      (entry) =>
        `<button data-act="tab" data-tab="${entry.key}" class="${tab === entry.key ? 'active' : ''}">${esc(entry.label)}</button>`,
    )
    .join('')

  $('#btn-poll').disabled = Boolean(state.meta.polling)
  $('#btn-poll').textContent = state.meta.polling ? '抓取中…' : '立即抓取'

  const view = $('#view')
  const scroll = view.scrollTop
  view.innerHTML =
    (lastError ? `<div class="error-bar">${esc(lastError)}</div>` : '') +
    (tab === 'notices'
      ? viewNotices()
      : tab === 'schedule'
        ? viewSchedule()
        : tab === 'sources'
          ? viewSources()
          : viewSettings())
  view.scrollTop = scroll
  delete view.dataset.skeleton
  // 仅在切换标签时播放进入动画：状态刷新（每次 api 后都会 render）不该反复闪动
  if (lastRenderedTab !== tab) {
    lastRenderedTab = tab
    view.classList.remove('view-enter')
    void view.offsetWidth // 强制重排，确保动画可重触发
    view.classList.add('view-enter')
  }
}

/** 首次读取数据前的骨架占位（避免白屏，加载完成后由 render 覆盖）。 */
function renderSkeleton() {
  const view = $('#view')
  if (!view || view.dataset.skeleton === '1') return
  view.dataset.skeleton = '1'
  view.innerHTML = Array.from({ length: 3 })
    .map(
      () => `<div class="skeleton-card" aria-hidden="true">
        <div class="skeleton skeleton-line w60"></div>
        <div class="skeleton skeleton-line w85"></div>
        <div class="skeleton skeleton-line w40"></div>
      </div>`,
    )
    .join('')
}

function viewNotices() {
  const notices = state.notices
  const unread = notices.filter((notice) => !notice.read).length
  const important = notices.filter((notice) => (notice.score || 0) >= 55).length
  const list = notices.filter((notice) => {
    if (noticeFilter === 'unread') return !notice.read
    if (noticeFilter === 'important') return (notice.score || 0) >= 55
    return true
  })
  // 计数与筛选永远基于全量 list；只有 DOM 渲染分段（首屏 100，之后每次 +200）。
  const visible = list.slice(0, noticeLimit)
  const rest = list.length - visible.length
  const attachmentIndex = indexAttachmentsByNotice(state.attachments)
  const chip = (key, label) =>
    `<button class="btn small ${noticeFilter === key ? 'primary' : 'ghost'}" data-act="filter" data-filter="${key}">${esc(label)}</button>`

  return `
    <div class="row">
      ${chip('all', `全部 ${notices.length}`)}
      ${chip('important', `重要 ${important}`)}
      ${chip('unread', `未读 ${unread}`)}
      <span class="spacer"></span>
      <button class="btn small ghost" data-act="notice-read-all" ${notices.length ? '' : 'disabled'}>全部标已读</button>
    </div>
    ${
      visible.length
        ? visible.map((notice) => noticeCard(notice, attachmentIndex)).join('') +
          (rest > 0
            ? `<div class="list-more"><button class="btn small primary" data-act="notice-show-more">显示更多（剩余 ${rest} 条）</button></div>`
            : '')
        : `<div class="empty">${
            notices.length
              ? '当前筛选下没有通知。'
              : '还没有抓到通知。\n先在「数据源」里启用或添加一个通知源，再点右上角「立即抓取」。'
          }</div>`
    }`
}

/**
 * 附件按通知 id 建索引，O(附件数)。
 * 全量通知下不能每条通知都 filter 一遍全表（1000×1000 会明显卡顿）；
 * 一条附件的 noticeIds 可能挂多条通知，所以一条附件可以进多个桶。
 * @returns {Map<string, object[]>}
 */
function indexAttachmentsByNotice(attachments) {
  const index = new Map()
  for (const item of Array.isArray(attachments) ? attachments : []) {
    for (const noticeId of item.noticeIds || []) {
      const bucket = index.get(noticeId)
      if (bucket) bucket.push(item)
      else index.set(noticeId, [item])
    }
  }
  return index
}

/**
 * 渲染单条通知的「附件(N)」区块：从调用方建好的索引里按 notice.id 取桶。
 * 无附件时返回空串。四态操作入口：打开本地 / 重试 / 打开原网页。
 * @param {{id: string}} notice
 * @param {Map<string, object[]>} [attachmentIndex] viewNotices() 建好的索引；不传则就地建（仅兜底）
 */
function noticeAttachments(notice, attachmentIndex) {
  const list = (attachmentIndex || indexAttachmentsByNotice(state.attachments)).get(notice.id) || []
  if (!list.length) return ''
  const rows = list
    .map((item) => {
      const spec = ATTACHMENT_STATUS[item.status] || {
        label: item.status || '未知',
        cls: '',
        act: '',
        action: '',
      }
      const size = item.status === 'downloaded' ? `<span class="attachment-size">${fmtSize(item.sizeBytes)}</span>` : ''
      const action = spec.act
        ? `<span class="attachment-actions"><button class="btn small ghost" data-act="${spec.act}" data-id="${esc(item.id)}" data-notice="${esc(notice.id)}">${esc(spec.action)}</button></span>`
        : ''
      const error = item.error && (item.status === 'failed' || item.status === 'missing')
        ? `<div class="attachment-error">${esc(item.error)}</div>`
        : ''
      return `<div class="attachment-row">
        <span class="attachment-name" title="${esc(item.fileName || item.sourceUrl)}">${esc(item.fileName || item.sourceUrl || '附件')}</span>
        <span class="chip ${spec.cls}">${esc(spec.label)}</span>
        ${size}
        ${action}
        ${error}
      </div>`
    })
    .join('')
  return `<div class="attachments"><div class="attachments-title">附件（${list.length}）</div>${rows}</div>`
}

function noticeCard(notice, attachmentIndex) {
  const tags = (notice.tags || [])
    .slice(0, 4)
    .map((tag) => `<span class="chip">${esc(tag)}</span>`)
    .join('')
  const expanded = expandedNotices.has(notice.id)
  const hasBody = (notice.content || '').length > 40
  const titleHtml = notice.url
    ? `<a class="notice-title ${notice.read ? 'read' : ''}" href="${esc(notice.url)}" target="_blank" rel="noreferrer" data-act="notice-open" data-id="${esc(notice.id)}">${esc(notice.title)}</a>`
    : `<span class="notice-title ${notice.read ? 'read' : ''}">${esc(notice.title)}</span>`
  return `
    <article class="card notice">
      <div class="notice-head">
        <i class="dot" style="background:${scoreTone(notice.score || 0)}" title="重要度 ${notice.score || 0}"></i>
        <div style="flex:1;min-width:0">
          ${titleHtml}
          <div class="meta">
            <span>${esc(notice.sourceName || '')}</span>
            ${notice.publishedAt ? `<span>· ${esc(notice.publishedAt)}</span>` : ''}
            ${notice.read ? '' : '<span class="chip accent">未读</span>'}
            ${notice.ai ? '<span class="chip accent">AI 摘要</span>' : ''}
            ${notice.deadline ? `<span class="chip warn">截止 ${esc(fmtDue(notice.deadline))}</span>` : ''}
            ${notice.contact ? `<span class="chip">找谁办 ${esc(notice.contact)}</span>` : ''}
            ${tags}
          </div>
          ${notice.summary ? `<p class="summary">${esc(notice.summary)}</p>` : ''}
          ${
            notice.useful?.length
              ? `<ul class="useful">${notice.useful.map((line) => `<li>${esc(line)}</li>`).join('')}</ul>`
              : ''
          }
          ${
            expanded && hasBody
              ? `<div class="fulltext">${esc(notice.content)}</div>`
              : ''
          }
        </div>
      </div>
      ${noticeAttachments(notice, attachmentIndex)}
      <div class="actions">
        ${
          hasBody
            ? `<button class="btn small ghost" data-act="notice-body" data-id="${esc(notice.id)}">${expanded ? '收起正文' : '展开正文'}</button>`
            : ''
        }
        <button class="btn small ghost" data-act="notice-read" data-id="${esc(notice.id)}" data-read="${notice.read ? '0' : '1'}">${notice.read ? '标未读' : '标已读'}</button>
        <button class="btn small ghost" data-act="notice-ai" data-id="${esc(notice.id)}">AI 重摘要</button>
        <button class="btn small primary" data-act="notice-schedule" data-id="${esc(notice.id)}">加入日程</button>
        <button class="btn small danger" data-act="notice-remove" data-id="${esc(notice.id)}">移除</button>
      </div>
    </article>`
}

function viewSchedule() {
  const items = state.items
  const active = items.filter((item) => !item.done)
  const done = items.filter((item) => item.done)
  const shown = showDone ? items : active
  const groups = []
  for (const label of ['已逾期', '今天', '明天', '本周内', '更远', '未定时间']) {
    const list = shown
      .filter((item) => !item.done && dayLabel(item.dueAt) === label)
      .sort((a, b) => String(a.dueAt).localeCompare(String(b.dueAt)))
    if (list.length) groups.push({ label, list })
  }
  if (showDone && done.length) {
    groups.push({
      label: '已完成',
      list: done.slice().sort((a, b) => String(b.doneAt || '').localeCompare(String(a.doneAt || ''))),
    })
  }

  return `
    <div class="row">
      <button class="btn primary" data-act="toggle-add">${openAdd ? '收起' : '＋ 手动添加日程'}</button>
      <label class="switch"><input type="checkbox" data-act="toggle-showdone" ${showDone ? 'checked' : ''} />显示已完成</label>
      <span class="spacer"></span>
      <span class="hint">共 ${active.length} 项待办</span>
    </div>
    ${openAdd ? addItemForm() : ''}
    ${
      groups.length
        ? groups
            .map(
              (group) => `
        <div class="group-label ${group.label === '已逾期' ? 'overdue' : ''}">${esc(group.label)} · ${group.list.length}</div>
        ${group.list.map(itemRow).join('')}`,
            )
            .join('')
        : `<div class="empty">还没有日程。\n点「＋ 手动添加日程」，或者在「通知」里把某条通知一键加入日程。</div>`
    }`
}

function addItemForm() {
  const draft = drafts.item
  return `
    <div class="card">
      <label class="field"><span>标题</span><input data-scope="item" data-field="title" value="${esc(draft.title)}" placeholder="例如：提交大学英语四级报名表" /></label>
      <div class="field-row">
        <label class="field"><span>时间</span><input type="datetime-local" data-scope="item" data-field="dueAt" value="${esc(draft.dueAt)}" /></label>
        <label class="field"><span>提前提醒（分钟，逗号分隔）</span><input data-scope="item" data-field="remind" value="${esc(draft.remind)}" placeholder="1440,120" /></label>
      </div>
      <label class="field"><span>备注</span><textarea data-scope="item" data-field="note" placeholder="可选：地点、要带的材料">${esc(draft.note)}</textarea></label>
      <div class="actions">
        <button class="btn ghost" data-act="toggle-add">取消</button>
        <button class="btn primary" data-act="item-add">加入日程</button>
      </div>
    </div>`
}

function itemRow(item) {
  const editing = drafts.editItem[item.id]
  const overdue = !item.done && (parseDue(item.dueAt)?.getTime() ?? Infinity) < Date.now()
  if (editing) {
    return `
      <div class="item">
        <div class="item-main">
          <label class="field"><span>标题</span><input data-scope="edit-item" data-id="${esc(item.id)}" data-field="title" value="${esc(editing.title)}" /></label>
          <div class="field-row">
            <label class="field"><span>时间</span><input type="datetime-local" data-scope="edit-item" data-id="${esc(item.id)}" data-field="dueAt" value="${esc(editing.dueAt)}" /></label>
            <label class="field"><span>提醒（分钟）</span><input data-scope="edit-item" data-id="${esc(item.id)}" data-field="remind" value="${esc(editing.remind)}" /></label>
          </div>
          <div class="actions">
            <button class="btn small ghost" data-act="item-edit-cancel" data-id="${esc(item.id)}">取消</button>
            <button class="btn small primary" data-act="item-edit-save" data-id="${esc(item.id)}">保存</button>
          </div>
        </div>
      </div>`
  }
  return `
    <div class="item ${item.done ? 'done' : ''}">
      <input type="checkbox" data-act="item-done" data-id="${esc(item.id)}" ${item.done ? 'checked' : ''} aria-label="完成" />
      <div class="item-main">
        <div class="row" style="gap:7px">
          <span class="item-title">${esc(item.title)}</span>
          <span class="chip ${overdue ? 'danger' : ''}">${esc(fmtDue(item.dueAt))}</span>
          ${item.done ? '' : `<span class="chip ${overdue ? 'danger' : 'accent'}">${esc(countdown(item.dueAt))}</span>`}
          <span class="chip">${(item.remindBefore || []).map(fmtMinutes).join('、') || '无提醒'}</span>
          ${item.source === 'notice' ? '<span class="chip accent">来自通知</span>' : ''}
        </div>
        ${item.note ? `<div class="hint">${esc(item.note)}</div>` : ''}
        ${
          item.url
            ? `<a class="hint" href="${esc(item.url)}" target="_blank" rel="noreferrer">查看通知原文 ↗</a>`
            : ''
        }
        <div class="actions" style="justify-content:flex-start">
          <button class="btn small ghost" data-act="item-edit" data-id="${esc(item.id)}">改时间</button>
          ${
            item.fired?.length
              ? `<button class="btn small ghost" data-act="item-refire" data-id="${esc(item.id)}">重置提醒</button>`
              : ''
          }
          <button class="btn small danger" data-act="item-remove" data-id="${esc(item.id)}">删除</button>
        </div>
      </div>
    </div>`
}

function viewSources() {
  const draft = drafts.sourceAdd
  return `
    <div class="row">
      <button class="btn primary" data-act="toggle-source-add">${openSourceAdd ? '收起' : '＋ 添加数据源'}</button>
      <span class="hint">把学校通知公告列表页的网址贴进来即可</span>
    </div>
    ${
      openSourceAdd
        ? `
      <div class="card">
        <div class="field-row">
          <label class="field"><span>名称（可留空）</span><input data-scope="source-add" data-field="name" value="${esc(draft.name)}" placeholder="例如：教务处通知" /></label>
          <label class="field" style="flex:2 1 260px"><span>列表页网址</span><input data-scope="source-add" data-field="url" value="${esc(draft.url)}" placeholder="https://…" /></label>
        </div>
        <div class="field-row">
          <label class="field"><span>解析方式</span>
            <select data-scope="source-add" data-field="type">
              <option value="html" ${draft.type === 'html' ? 'selected' : ''}>HTML 列表页</option>
              <option value="rss" ${draft.type === 'rss' ? 'selected' : ''}>RSS / Atom</option>
              <option value="json" ${draft.type === 'json' ? 'selected' : ''}>JSON 接口</option>
            </select>
          </label>
          <label class="field"><span>DNS 覆盖 IP（可选）</span><input data-scope="source-add" data-field="resolveIp" value="${esc(draft.resolveIp)}" placeholder="域名解析不到时填" /></label>
        </div>
        <label class="field"><span>登录态 Cookie（可选）</span><textarea data-scope="source-add" data-field="cookie" placeholder="从浏览器 F12 的 Network 里复制整条 Cookie">${esc(draft.cookie)}</textarea></label>
        <label class="switch"><input type="checkbox" data-act="source-add-llm" ${draft.sendToLlm !== false ? 'checked' : ''} />该源正文可送 AI 摘要（学籍 / 成绩等敏感源建议关闭）</label>
        <div class="actions">
          <button class="btn ghost" data-act="toggle-source-add">取消</button>
          <button class="btn primary" data-act="source-add">添加</button>
        </div>
      </div>`
        : ''
    }
    ${state.sources.map(sourceCard).join('')}
    <div class="hint">
      校内门户通常只在校内网络可访问；校外一般要走学校 WebVPN，并需要把浏览器的 Cookie 粘到「登录态」里。<br />
      解析不到域名时，可以在「DNS 覆盖 IP」里手填服务器 IP（插件会保留原域名做 TLS 校验）。<br />
      纯前端渲染的门户请把解析方式改成 JSON，并在编辑里填接口地址与字段映射。<br />
  这里的「正常」= 程序用它自己保存的 Cookie 抓取成功。你在浏览器里直接打开接口地址（如 <code>get_news_list?id=…</code>）看到「请登录」是正常的——那是浏览器自己的会话，和程序存的不是同一份。
    </div>`
}

function sourceCard(source) {
  const editing = drafts.editSource[source.id]
  const tone = source.status === 'ok' ? 'ok' : source.status === 'error' ? 'danger' : ''
  const statusText =
    source.status === 'ok'
      ? `正常 · ${source.statusText || ''}`
      : source.status === 'error'
        ? `失败 · ${source.statusText || ''}`
        : '尚未抓取'

  if (editing) {
    return `
      <div class="card">
        <div class="row"><span class="chip">${esc(source.name)}</span><span class="chip">编辑中</span></div>
        <label class="field"><span>名称</span><input data-scope="edit-source" data-id="${esc(source.id)}" data-field="name" value="${esc(editing.name)}" /></label>
        <label class="field"><span>列表页网址</span><input data-scope="edit-source" data-id="${esc(source.id)}" data-field="url" value="${esc(editing.url)}" /></label>
        <div class="field-row">
          <label class="field"><span>解析方式</span>
            <select data-scope="edit-source" data-id="${esc(source.id)}" data-field="type">
              <option value="html" ${editing.type === 'html' ? 'selected' : ''}>HTML 列表页</option>
              <option value="rss" ${editing.type === 'rss' ? 'selected' : ''}>RSS / Atom</option>
              <option value="json" ${editing.type === 'json' ? 'selected' : ''}>JSON 接口</option>
            </select>
          </label>
          <label class="field"><span>DNS 覆盖 IP</span><input data-scope="edit-source" data-id="${esc(source.id)}" data-field="resolveIp" value="${esc(editing.resolveIp)}" /></label>
        </div>
        <label class="field"><span>登录态 Cookie</span><textarea data-scope="edit-source" data-id="${esc(source.id)}" data-field="cookie" placeholder="留空表示不需要登录">${esc(editing.cookie)}</textarea></label>
        <label class="switch"><input type="checkbox" data-act="source-edit-llm" data-id="${esc(source.id)}" ${editing.sendToLlm !== false ? 'checked' : ''} />该源正文可送 AI 摘要（学籍 / 成绩等敏感源建议关闭）</label>
        <div class="field-row">
          <label class="field"><span>翻页模板（{n} 表示页码）</span><input data-scope="edit-source" data-id="${esc(source.id)}" data-field="pageUrl" value="${esc(editing.pageUrl)}" placeholder="index_{n}.htm" /></label>
          <label class="field" style="flex:0 0 90px"><span>页数</span><input data-scope="edit-source" data-id="${esc(source.id)}" data-field="pages" value="${esc(editing.pages)}" /></label>
        </div>
        <label class="field"><span>只保留匹配这些链接（正则，可选）</span><input data-scope="edit-source" data-id="${esc(source.id)}" data-field="linkPattern" value="${esc(editing.linkPattern)}" placeholder="info/10 或 tzgg" /></label>
        <label class="field"><span>备注</span><input data-scope="edit-source" data-id="${esc(source.id)}" data-field="note" value="${esc(editing.note)}" /></label>
        <div class="actions">
          <button class="btn ghost" data-act="source-edit-cancel" data-id="${esc(source.id)}">取消</button>
          <button class="btn primary" data-act="source-edit-save" data-id="${esc(source.id)}">保存</button>
        </div>
      </div>`
  }

  return `
    <div class="card">
      <div class="row">
        <label class="switch"><input type="checkbox" data-act="source-toggle" data-id="${esc(source.id)}" ${source.enabled ? 'checked' : ''} /></label>
        <span style="font-weight:600">${esc(source.name)}</span>
        <span class="chip">${esc(source.type || 'html')}</span>
        <span class="chip ${tone}">${esc(statusText)}</span>
        ${
          source.loginExpired
            ? '<span class="chip danger" title="存的 Cookie 已经被拒绝：点「测试」会自动重新登录，或手动更新登录态">Cookie 已失效</span>'
            : source.hasCookie
              ? '<span class="chip ok" title="已存 Cookie；是否还有效看左边的状态">已存 Cookie</span>'
              : ''
        }
        <span class="spacer"></span>
        <button class="btn small ghost" data-act="source-test" data-id="${esc(source.id)}">测试</button>
        <button class="btn small ghost" data-act="source-edit" data-id="${esc(source.id)}">编辑</button>
        <button class="btn small danger" data-act="source-remove" data-id="${esc(source.id)}">删除</button>
      </div>
      <a class="hint" href="${esc(source.url)}" target="_blank" rel="noreferrer" style="word-break:break-all">${esc(source.url)}</a>
      ${source.note ? `<div class="hint">${esc(source.note)}</div>` : ''}
    </div>`
}

function viewSettings() {
  if (!drafts.settings) {
    const settings = state.settings || {}
    drafts.settings = {
      minutes: String(settings.pollMinutes ?? 30),
      keywords: (settings.keywords || []).join('、'),
      remind: (settings.autoRemindBefore || []).join(','),
      key: '',
      accountCas: (state.meta?.account?.casBase || '').toString(),
      accountService: (state.meta?.account?.service || '').toString(),
      accountUser: (state.meta?.account?.username || '').toString(),
      accountPassword: '',
      webvpnCookie: '',
      dateRangeStart: (settings.dateRangeStart || '').toString(),
      dateRangeEnd: (settings.dateRangeEnd || '').toString(),
    }
  }
  const draft = drafts.settings
  const meta = state.meta || {}
  const rangePreset = matchRangePreset(draft.dateRangeStart, draft.dateRangeEnd)
  const presetButton = (days, label) => {
    const on = rangePreset === String(days)
    return `<button type="button" class="btn small ghost${on ? ' is-active' : ''}" data-act="settings-range-preset" data-days="${days}" aria-pressed="${on}">${label}</button>`
  }
  return `
    <div class="card">
      <div class="field-row">
        <label class="field"><span>抓取间隔（分钟，最小 5）</span><input data-scope="settings" data-field="minutes" value="${esc(draft.minutes)}" /></label>
        <label class="field"><span>自动建日程的提醒档位（分钟）</span><input data-scope="settings" data-field="remind" value="${esc(draft.remind)}" placeholder="1440,120" /></label>
      </div>
      <div class="range-presets" role="group" aria-label="默认抓取范围快捷选择">
        ${presetButton(7, '近 7 天')}${presetButton(30, '近 30 天')}${presetButton(90, '近 90 天')}${presetButton('custom', '自定义')}
      </div>
      <div class="field-row">
        <label class="field"><span>默认抓取起始日期</span><input type="date" data-scope="settings" data-field="dateRangeStart" value="${esc(draft.dateRangeStart)}" /></label>
        <label class="field"><span>默认抓取结束日期</span><input type="date" data-scope="settings" data-field="dateRangeEnd" value="${esc(draft.dateRangeEnd)}" /></label>
      </div>
      <div class="row"><button class="btn small ghost" data-act="settings-clear-range">清除日期范围</button></div>
      <div class="hint">两端留空表示不限制；仅抓取发布日期落在区间内的通知。此处为默认值，抓取时可在浮窗临时覆盖。</div>
      <label class="field"><span>关注关键词（顿号或逗号分隔，命中越多越重要）</span><textarea data-scope="settings" data-field="keywords">${esc(draft.keywords)}</textarea></label>
      <div class="actions">
        <button class="btn primary" data-act="settings-save">保存设置</button>
      </div>
    </div>

    <div class="section-title">行为</div>
    <div class="card">
      <label class="switch"><input type="checkbox" data-act="toggle-attach" ${state.settings.fetchAttachments ? 'checked' : ''} />自动抓取附件（白名单文件下载到本地，删除通知时一并清理）</label>
      <label class="switch"><input type="checkbox" data-act="toggle-llm" ${state.settings.llm ? 'checked' : ''} />新通知用 AI 摘要（关掉后走规则摘要）</label>
      <label class="switch"><input type="checkbox" data-act="llm-consent" ${localStorage.getItem('llmConsentAt') ? 'checked' : ''} />我已知晓：开启 AI 后通知正文将发送至配置的模型服务（llm.baseURL）用于摘要与分类，日志不含正文（§6.4）</label>
      <label class="switch"><input type="checkbox" data-act="toggle-autoschedule" ${state.settings.autoSchedule ? 'checked' : ''} />通知里识别到截止时间时，自动建成日程</label>
      <label class="switch"><input type="checkbox" data-act="toggle-detail" ${state.settings.fetchDetail ? 'checked' : ''} />跟进详情页正文（更准，但每次多几个请求）</label>
      <label class="switch"><input type="checkbox" data-act="toggle-notify" ${state.settings.notify !== false ? 'checked' : ''} />到点弹桌面通知（Windows 系统通知）</label>
    </div>

    <div class="section-title">校外网络（WebVPN）</div>
    <div class="card">
      <label class="switch"><input type="checkbox" data-act="toggle-webvpn" ${state.settings.webvpn?.enabled ? 'checked' : ''} />开启校外模式：列表、翻页、详情全部走学校 WebVPN</label>
      <div class="hint">
        BJUT 专用：网关地址已内置为 <code>${esc(state.settings.webvpn?.gateway || '')}</code>，地址重写规则也已内置，<b>无需填写任何配置</b>。
        校外直连不到校内门户时打开本开关。开启后程序把每个出站地址改写成网关地址，响应里被网关改写过的链接再还原成门户原始地址，
        所以数据源里的 <code>url</code> / <code>pageUrl</code> / <code>detailUrlTemplate</code> <b>保持校内原始地址，不用手改</b>。关掉开关后与校内直连完全一致。
      </div>
      <label class="field"><span>网关会话 Cookie${state.settings.webvpn?.hasCookie ? '（已保存，留空不修改）' : ''}</span><textarea data-scope="settings" data-field="webvpnCookie" placeholder="浏览器登录 WebVPN 后，从 F12 → Network 复制整条 Cookie">${esc(draft.webvpnCookie)}</textarea></label>
      <div class="actions">
        <button class="btn primary" data-act="webvpn-login">窗口登录 WebVPN</button>
        <button class="btn ghost" data-act="webvpn-clear-cookie">清除网关 Cookie</button>
        <button class="btn ghost" data-act="webvpn-save">保存网关 Cookie</button>
      </div>
    </div>

    <div class="section-title">自动登录（统一身份认证）</div>
    <div class="card">
      <div class="row" style="align-items:center">
        <span class="chip ${meta.account?.configured ? 'ok' : 'warn'}">${meta.account?.configured ? '已配置账号' : '未配置'}</span>
        <button class="btn small primary" data-act="open-login">登录向导</button>
        <span class="hint">三模式：窗口登录（推荐，支持验证码）/ 账号密码 / 粘贴 Cookie</span>
      </div>
      <div class="hint">
        5 个常见失败的下一步：断网→检查网络后重试；密码错→重新输入，连续 3 次改窗口登录；
        验证码→改用窗口登录；会话过期→点「重新登录」自动续期；网关过期→到「校外网络」更新网关 Cookie。
      </div>
      <div class="hint">
        填一次账号密码，登录态过期时程序会自己走统一身份认证、把新 Cookie 写回同一门户下的数据源，不用再手动复制。
        <br />「认证入口地址」填登录页所在的站点（一般以 <code>/login</code> 结尾）；「登录后返回的地址」要填门户的认证回调，不是首页——通常形如 <code>https://门户/common/actionCasLogin?redirect_url=https://门户/首页</code>。填成首页时登录会"成功"，但门户不下发会话 Cookie，抓取照样提示要登录（程序会自动换成回调地址重试并记住）。
        <br />密码用 <b>Windows DPAPI</b> 加密存在本机的 <code>config.json</code>，只有当前 Windows 用户能解开；换电脑或换用户需要重填。遇到验证码会自动退回「手动更新登录态」。
      </div>
      <div class="field-row">
        <label class="field"><span>认证入口地址</span><input data-scope="settings" data-field="accountCas" value="${esc(draft.accountCas)}" placeholder="https://cas.example.edu.cn" /></label>
        <label class="field"><span>登录后返回的地址</span><input data-scope="settings" data-field="accountService" value="${esc(draft.accountService)}" placeholder="https://portal.example.edu.cn/" /></label>
      </div>
      <div class="field-row">
        <label class="field"><span>学号 / 账号</span><input data-scope="settings" data-field="accountUser" value="${esc(draft.accountUser)}" placeholder="学号或工号" /></label>
        <label class="field"><span>密码${meta.account?.configured ? '（已保存，留空则不修改）' : ''}</span><input type="password" data-scope="settings" data-field="accountPassword" value="" placeholder="只在保存时传一次" autocomplete="new-password" /></label>
      </div>
      <div class="actions">
        <button class="btn ghost" data-act="account-test">测试自动登录</button>
        <button class="btn ghost" data-act="account-clear">清除已保存</button>
        <button class="btn primary" data-act="account-save">保存并登录</button>
      </div>
      <div class="hint">
        ${meta.account?.configured ? '当前：已保存账号密码' : '当前：未保存（只能用各数据源里的 Cookie）'}
        ${
          meta.account?.lastLogin?.at
            ? `<br>上次自动登录：${
                meta.account.lastLogin.ok === true
                  ? '成功'
                  : meta.account.lastLogin.ok === false
                    ? `失败（${esc(meta.account.lastLogin.message)}）`
                    : esc(meta.account.lastLogin.message)
              } · ${esc(String(meta.account.lastLogin.at).replace('T', ' ').slice(0, 16))}`
            : ''
        }
      </div>
    </div>

    <div class="section-title">AI 摘要</div>
    <div class="card">
      <div class="hint">填 DeepSeek 的 API Key（也可以设环境变量 DEEPSEEK_API_KEY）。${meta.llmKeySet ? '当前已配置。' : '当前未配置，只能用规则摘要。'}</div>
      <label class="field"><span>API Key</span><input type="password" data-scope="settings" data-field="key" value="${esc(draft.key)}" placeholder="${meta.llmKeySet ? '已保存（留空不修改）' : 'sk-…'}" /></label>
      <div class="actions">
        <button class="btn ghost" data-act="llm-test">测试模型</button>
        <button class="btn primary" data-act="llm-save">保存 Key</button>
      </div>
      ${
        meta.llm?.at
          ? `<div class="hint">最近一次：${meta.llm.ok ? '正常' : `失败（${esc(meta.llm.message)}）`} · ${esc(String(meta.llm.at).replace('T', ' ').slice(0, 16))}</div>`
          : ''
      }
      <div class="hint">本月已用 ${Number(meta.llmUsed || 0)} tokens · 月度上限 ${
        Number(meta.llmBudget || 0) > 0 ? `${Number(meta.llmBudget)} tokens` : '不限（超限自动只走规则摘要）'
      }</div>
    </div>

    <div class="section-title">状态</div>
    <div class="card">
      <div class="hint">上次抓取：${esc(meta.lastPollAt ? String(meta.lastPollAt).replace('T', ' ').slice(0, 16) : '从未')} · 累计 ${meta.pollCount || 0} 次</div>
      ${meta.lastPollError ? `<div class="error-bar">${esc(meta.lastPollError)}</div>` : ''}
      <div class="hint">数据文件：${esc(meta.dataPath || '')}</div>
      <div class="hint">服务端口：${esc(meta.port || '')} · 关闭窗口时可选「最小化到托盘」（继续后台抓取与提醒）或「完全退出」；托盘菜单选「退出」也可完全退出</div>
      <div class="hint">RSS 订阅地址（手机阅读器可直接订阅）：<code>http://127.0.0.1:${esc(meta.port || '')}/rss.xml</code>，只看重要的加 <code>?minScore=40</code></div>
      <div class="actions">
        <button class="btn ghost" data-act="notify-test">测试桌面通知</button>
        <button class="btn ghost" data-act="rescore">按当前关键词重算摘要</button>
        <button class="btn ghost" data-act="refill">补齐通知正文</button>
      </div>
    </div>`
}

/* ------------------------------ 事件 ------------------------------ */

function readDrafts(scope, field, value, id) {
  if (scope === 'item') drafts.item[field] = value
  else if (scope === 'source-add') drafts.sourceAdd[field] = value
  else if (scope === 'settings' && drafts.settings) drafts.settings[field] = value
  else if (scope === 'edit-item' && drafts.editItem[id]) drafts.editItem[id][field] = value
  else if (scope === 'edit-source' && drafts.editSource[id]) drafts.editSource[id][field] = value
}

document.addEventListener('input', (event) => {
  const el = event.target.closest('[data-scope]')
  if (!el) return
  const value = el.type === 'checkbox' ? el.checked : el.value
  readDrafts(el.dataset.scope, el.dataset.field, value, el.dataset.id)
})

document.addEventListener('change', (event) => {
  const el = event.target.closest('[data-scope]')
  if (el) {
    const value = el.type === 'checkbox' ? el.checked : el.value
    readDrafts(el.dataset.scope, el.dataset.field, value, el.dataset.id)
    // 手动改默认抓取日期 = 自定义：只刷快捷项选中态，不落库、不重渲染（避免打断输入）
    if (el.dataset.scope === 'settings' && /^dateRange(Start|End)$/.test(el.dataset.field || '')) {
      syncRangePresetState('settings')
    }
    return
  }
  const toggle = event.target.closest('input[type="checkbox"][data-act]')
  if (!toggle) return
  const act = toggle.dataset.act
  const id = toggle.dataset.id
  if (act === 'toggle-showdone') {
    showDone = toggle.checked
    render(true)
  } else if (act === 'source-toggle') {
    api('source.update', { id, enabled: toggle.checked }, { quiet: true })
  } else if (act === 'source-add-llm') {
    if (drafts.sourceAdd) drafts.sourceAdd.sendToLlm = toggle.checked
  } else if (act === 'source-edit-llm') {
    if (drafts.editSource[id]) drafts.editSource[id].sendToLlm = toggle.checked
  } else if (act === 'llm-consent') {
    // §6.4 显式同意：勾选才允许开启 AI；正文将发送至 llm.baseURL。
    if (toggle.checked) localStorage.setItem('llmConsentAt', new Date().toISOString())
    else localStorage.removeItem('llmConsentAt')
  } else if (act === 'toggle-webvpn') {
    api('settings.save', { settings: { webvpn: { enabled: toggle.checked } } }, { quiet: true })
  } else if (act === 'toggle-llm') {
    if (toggle.checked && !localStorage.getItem('llmConsentAt')) {
      toggle.checked = false
      toast('error', '需要先同意隐私说明', '勾选下方「我已知晓」后再开启 AI 摘要')
      return
    }
    api('settings.save', { settings: { llm: toggle.checked } }, { quiet: true })
  } else if (act === 'toggle-autoschedule') {
    api('settings.save', { settings: { autoSchedule: toggle.checked } }, { quiet: true })
  } else if (act === 'toggle-detail') {
    api('settings.save', { settings: { fetchDetail: toggle.checked } }, { quiet: true })
  } else if (act === 'toggle-attach') {
    api('settings.save', { settings: { fetchAttachments: toggle.checked } }, { quiet: true })
  } else if (act === 'toggle-notify') {
    api('settings.save', { settings: { notify: toggle.checked } }, { quiet: true })
  } else if (act === 'item-done') {
    api('item.done', { id, done: toggle.checked }, { quiet: true })
  }
})

document.addEventListener('click', async (event) => {
  const button = event.target.closest('[data-act]')
  if (!button) return
  const act = button.dataset.act
  const id = button.dataset.id

  switch (act) {
    case 'tab':
      tab = button.dataset.tab
      // 切 tab 回到首屏 100 条：换到通知页时不该继承上一次"显示更多"的深度
      noticeLimit = NOTICE_PAGE_FIRST
      render(true)
      break
    case 'filter':
      noticeFilter = button.dataset.filter
      // 筛选取的是另一批数据，已渲染深度在这里没有意义，回到 100
      noticeLimit = NOTICE_PAGE_FIRST
      render(true)
      break
    case 'notice-show-more':
      noticeLimit += NOTICE_PAGE_MORE
      render(true)
      // 重渲染换掉了节点；按钮若还在就把焦点接回去，键盘用户不用再 Tab 到底
      $('[data-act="notice-show-more"]')?.focus()
      break
    case 'toast-close': {
      dismissToast(id)
      break
    }
    case 'poll':
      openPollRange()
      break
    case 'poll-range-cancel':
      closePollRange()
      break
    case 'poll-range-clear': {
      const startEl = $('#poll-range-start')
      const endEl = $('#poll-range-end')
      if (startEl) startEl.value = ''
      if (endEl) endEl.value = ''
      syncRangePresetState('poll')
      break
    }
    case 'poll-range-preset':
      // 快捷选择只填输入框，抓取仍由「开始抓取」确认，避免误触直接开跑。
      applyRangePreset(button.dataset.days, $('#poll-range-start'), $('#poll-range-end'))
      syncRangePresetState('poll')
      break
    case 'settings-range-preset': {
      if (!drafts.settings) break
      const days = button.dataset.days
      if (days === 'custom') {
        $('#view [data-field="dateRangeStart"]')?.focus()
        break
      }
      const range = presetRange(days)
      if (!range) break
      // 只改草稿并重渲染，仍要用户点「保存设置」才落库。
      drafts.settings.dateRangeStart = range.start
      drafts.settings.dateRangeEnd = range.end
      render(true)
      // 重渲染会换掉节点，把焦点交回同一快捷项，键盘流程不中断
      $(`#view [data-act="settings-range-preset"][data-days="${days}"]`)?.focus()
      break
    }
    case 'poll-range-confirm': {
      const start = ($('#poll-range-start')?.value || '').trim()
      const end = ($('#poll-range-end')?.value || '').trim()
      const check = validateDateRange(start, end)
      if (!check.ok) {
        toast('error', '日期范围不正确', check.error)
        break
      }
      if (check.soft) toast('ok', '提示', check.soft)
      closePollRange()
      // 临时范围只随本次 poll 下发，绝不写回设置默认值。
      // 抓取可能持续数十秒：顶部按钮进入处理中态，进度条走不确定态，避免「点了没反应」。
      const pollBtn = $('#btn-poll')
      const bar = $('#poll-progress')
      pollBtn?.classList.add('is-busy')
      if (bar) {
        bar.hidden = false
        bar.classList.add('is-indeterminate')
      }
      try {
        await api('poll', { dateRange: { start, end } }, { quiet: false })
      } finally {
        pollBtn?.classList.remove('is-busy')
        if (bar) {
          bar.classList.remove('is-indeterminate')
          bar.hidden = true
        }
      }
      break
    }
    case 'attachment-open': {
      const attachment = (state.attachments || []).find((entry) => entry.id === id)
      const path = attachment?.localPath
      if (!path) {
        toast('error', '打不开附件', '本地路径不存在，可尝试重试')
        break
      }
      try {
        await openAttachment(path)
      } catch (error) {
        toast('error', '打不开附件', String(error?.message || error))
      }
      break
    }
    case 'attachment-retry':
      await withBusy(button, '重试中…', () => api('attachment.retry', { id }, { quiet: false }))
      break
    case 'attachment-source': {
      const attachment = (state.attachments || []).find((entry) => entry.id === id)
      if (attachment?.sourceUrl) openExternal(attachment.sourceUrl)
      else toast('error', '没有原网页地址', '该附件没有来源链接')
      break
    }
    case 'notice-open':
      api('notice.read', { id, read: true }, { quiet: true })
      break
    case 'notice-body':
      if (expandedNotices.has(id)) expandedNotices.delete(id)
      else expandedNotices.add(id)
      render(true)
      break
    case 'notice-read':
      api('notice.read', { id, read: button.dataset.read === '1' }, { quiet: true })
      break
    case 'notice-read-all':
      await withBusy(button, '处理中…', () => api('notice.readAll', {}, { quiet: true }))
      break
    case 'notice-remove':
      await withBusy(button, '移除中…', () => api('notice.remove', { id }, { quiet: true }))
      break
    case 'notice-ai':
      await withBusy(button, '摘要中…', () => api('notice.summarize', { id }))
      break
    case 'notice-schedule': {
      const notice = state.notices.find((entry) => entry.id === id)
      if (!notice) break
      const tomorrow = new Date(Date.now() + 86400000)
      const fallback = `${tomorrow.getFullYear()}-${pad2(tomorrow.getMonth() + 1)}-${pad2(tomorrow.getDate())}T09:00`
      await api('item.add', {
        title: notice.title.slice(0, 60),
        note: `来自 ${notice.sourceName}：${notice.summary || ''}`,
        dueAt: notice.deadline || fallback,
        noticeId: notice.id,
        url: notice.url,
        source: 'notice',
      })
      break
    }
    case 'toggle-add':
      openAdd = !openAdd
      render(true)
      break
    case 'item-add': {
      const draft = drafts.item
      if (!draft.title.trim()) return toast('error', '缺少标题', '先写清楚要做的事')
      if (!draft.dueAt) return toast('error', '缺少时间', '选一个截止时间')
      const result = await withBusy(button, '添加中…', () =>
        api('item.add', {
          title: draft.title,
          dueAt: draft.dueAt,
          note: draft.note,
          remindBefore: draft.remind
            .split(/[,，\s]+/)
            .map(Number)
            .filter((value) => Number.isFinite(value) && value >= 0),
        }),
      )
      if (result?.ok) {
        drafts.item = { title: '', dueAt: '', note: '', remind: '1440,120' }
        openAdd = false
        render(true)
      }
      break
    }
    case 'item-edit': {
      const item = state.items.find((entry) => entry.id === id)
      if (!item) break
      drafts.editItem[id] = {
        title: item.title,
        dueAt: toLocalInput(item.dueAt),
        remind: (item.remindBefore || []).join(','),
      }
      render(true)
      break
    }
    case 'item-edit-cancel':
      delete drafts.editItem[id]
      render(true)
      break
    case 'item-edit-save': {
      const draft = drafts.editItem[id]
      if (!draft) break
      await withBusy(button, '保存中…', () =>
        api('item.update', {
          id,
          title: draft.title,
          dueAt: draft.dueAt,
          remindBefore: draft.remind
            .split(/[,，\s]+/)
            .map(Number)
            .filter((value) => Number.isFinite(value) && value >= 0),
        }),
      )
      delete drafts.editItem[id]
      render(true)
      break
    }
    case 'item-refire':
      await withBusy(button, '处理中…', () => api('item.refire', { id }, { quiet: true }))
      break
    case 'item-remove':
      await withBusy(button, '移除中…', () => api('item.remove', { id }, { quiet: true }))
      break
    case 'toggle-source-add':
      openSourceAdd = !openSourceAdd
      render(true)
      break
    case 'source-add': {
      const draft = drafts.sourceAdd
      if (!/^https?:\/\//i.test(draft.url.trim())) return toast('error', '网址不对', '要以 http:// 或 https:// 开头')
      const result = await withBusy(button, '添加中…', () =>
        api('source.add', {
          name: draft.name,
          url: draft.url,
          type: draft.type,
          cookie: draft.cookie,
          resolveIp: draft.resolveIp,
          sendToLlm: draft.sendToLlm !== false,
        }),
      )
      if (result?.ok) {
        drafts.sourceAdd = { name: '', url: '', type: 'html', cookie: '', resolveIp: '', note: '', sendToLlm: true }
        openSourceAdd = false
        render(true)
      }
      break
    }
    case 'source-edit': {
      const source = state.sources.find((entry) => entry.id === id)
      if (!source) break
      drafts.editSource[id] = {
        name: source.name,
        url: source.url,
        type: source.type || 'html',
        resolveIp: source.resolveIp || '',
        linkPattern: source.linkPattern || '',
        pageUrl: source.pageUrl || '',
        urlTemplate: source.urlTemplate || '',
        detailUrlTemplate: source.detailUrlTemplate || '',
        contentPath: source.contentPath || '',
        pages: String(source.pages || 1),
        note: source.note || '',
        cookie: '',
        sendToLlm: source.sendToLlm !== false,
      }
      render(true)
      break
    }
    case 'source-edit-cancel':
      delete drafts.editSource[id]
      render(true)
      break
    case 'source-edit-save': {
      const draft = drafts.editSource[id]
      if (!draft) break
      const payload = {
        id,
        name: draft.name,
        url: draft.url,
        type: draft.type,
        resolveIp: draft.resolveIp,
        linkPattern: draft.linkPattern,
        pageUrl: draft.pageUrl,
        urlTemplate: draft.urlTemplate,
        detailUrlTemplate: draft.detailUrlTemplate,
        contentPath: draft.contentPath,
        pages: Number(draft.pages) || 1,
        note: draft.note,
        sendToLlm: draft.sendToLlm !== false,
      }
      if (draft.cookie) payload.cookie = draft.cookie
      await withBusy(button, '保存中…', () => api('source.update', payload))
      delete drafts.editSource[id]
      render(true)
      break
    }
    case 'source-test':
      await withBusy(button, '测试中…', () => api('source.test', { id }))
      break
    case 'source-remove':
      await withBusy(button, '移除中…', () => api('source.remove', { id }, { quiet: true }))
      break
    case 'settings-clear-range':
      if (drafts.settings) {
        drafts.settings.dateRangeStart = ''
        drafts.settings.dateRangeEnd = ''
      }
      render(true)
      toast('ok', '已清空日期范围', '点「保存设置」后生效（两端留空 = 不限时间）')
      break
    case 'settings-save': {
      const draft = drafts.settings
      const check = validateDateRange(draft.dateRangeStart, draft.dateRangeEnd)
      if (!check.ok) {
        toast('error', '日期范围不正确', check.error)
        break
      }
      if (check.soft) toast('ok', '提示', check.soft)
      await withBusy(button, '保存中…', () =>
        api('settings.save', {
          settings: {
            pollMinutes: Number(draft.minutes),
            keywords: draft.keywords.split(/[、,，\s]+/).map((word) => word.trim()).filter(Boolean),
            autoRemindBefore: draft.remind
              .split(/[,，\s]+/)
              .map(Number)
              .filter((value) => Number.isFinite(value) && value >= 0),
            dateRangeStart: (draft.dateRangeStart || '').trim(),
            dateRangeEnd: (draft.dateRangeEnd || '').trim(),
          },
        }),
      )
      break
    }
    case 'webvpn-login': {
      await withBusy(button, '正在打开登录窗…', async () => {
        try {
          const ticket = await authWebvpnLogin()
          // 先复位向导，再切到网关 scope；后端 auth://progress 会继续驱动状态。
          openWizard()
          wizard.ticket = ticket?.id || ''
          wizard.scope = 'gateway'
          wizard.mode = 'WebView'
          wizard.state = 'awaitingUser'
          wizard.stepIndex = 1
          wizard.error = null
          renderWizard()
          startWizardPolling()
        } catch (error) {
          toast('error', '打不开登录窗', String(error?.message || error))
        }
      })
      break
    }
    case 'webvpn-save': {
      const draft = drafts.settings
      if (!draft) break
      const cookie = (draft.webvpnCookie || '').trim()
      draft.webvpnCookie = ''
      await withBusy(button, '保存中…', () =>
        api('settings.save', { settings: { webvpn: cookie ? { cookie } : {} } }),
      )
      break
    }
    case 'webvpn-clear-cookie': {
      if (drafts.settings) drafts.settings.webvpnCookie = ''
      await withBusy(button, '清除中…', () =>
        api('settings.save', { settings: { webvpn: { clearCookie: true } } }),
      )
      break
    }
    case 'llm-save': {
      const key = (drafts.settings?.key || '').trim()
      if (!key) return toast('error', 'Key 是空的', '要么填一个，要么用环境变量')
      const result = await withBusy(button, '保存中…', () => api('llm.key', { apiKey: key }))
      if (result?.ok) drafts.settings.key = ''
      break
    }
    case 'llm-test': {
      await withBusy(button, '测试中…', () => api('llm.test'))
      break
    }
    case 'account-save': {
      const draft = drafts.settings || {}
      const casBase = (draft.accountCas || '').trim()
      const service = (draft.accountService || '').trim()
      const username = (draft.accountUser || '').trim()
      const password = draft.accountPassword || ''
      if (!casBase || !service) return toast('error', '缺认证地址', '填一下认证入口和登录后返回的地址')
      if (!username) return toast('error', '缺账号', '填一下学号或账号')
      if (!password && !state?.meta?.account?.configured) return toast('error', '缺密码', '第一次保存需要填密码')
      const result = await withBusy(button, '登录中…', () =>
        api('account.save', { casBase, service, username, password }),
      )
      if (result?.ok) {
        drafts.settings.accountPassword = ''
        toast('ok', '自动登录可用', result.message || '已保存账号密码')
      }
      break
    }
    case 'account-test': {
      const result = await withBusy(button, '登录中…', () => api('account.test'))
      if (result?.ok) toast('ok', '自动登录成功', result.message || '')
      break
    }
    case 'account-clear':
      await api('account.clear')
      if (drafts.settings) {
        drafts.settings.accountCas = ''
        drafts.settings.accountService = ''
        drafts.settings.accountUser = ''
        drafts.settings.accountPassword = ''
      }
      break
    case 'notify-test':
      await withBusy(button, '测试中…', () => api('notify.test'))
      break
    case 'rescore':
      await withBusy(button, '重算中…', () => api('notices.rescore'))
      break
    case 'refill':
      await withBusy(button, '补齐中…', () => api('notices.refill', { limit: 40 }))
      break
    case 'open-login':
      openWizard()
      break
    case 'wizard-cancel':
      await authCancel(wizard.ticket)
      wizard.state = 'cancelled'
      renderWizard()
      setTimeout(closeWizard, 1200)
      stopWizardPolling()
      break
    case 'wizard-mode':
      wizard.mode = button.dataset.mode || 'WebView'
      renderWizard()
      break
    case 'wizard-primary': {
      const spec = WIZARD_STATES[wizard.state] || {}
      if (wizard.state === 'awaitingUser' && wizard.mode === 'ImportCookie') {
        const text = ($('#wizard-cookie-text')?.value || '').trim()
        if (!text) return toast('error', 'Cookie 为空', '粘贴浏览器里的整条 Cookie')
        const result = await authImportCookie(wizard.scope, text)
        if (result?.state) applyState(result.state)
        if (result?.ok) {
          wizard.state = 'active'
          wizard.error = null
          closeWizard()
          stopWizardPolling()
          toast('ok', '已导入登录态', '下次抓取会自动使用')
        } else {
          wizard.state = 'failed'
          wizard.error = {
            code: 'AUTH_E_CREDENTIAL',
            message: result?.error || '导入失败',
            next: AUTH_ERROR_COPY.AUTH_E_CREDENTIAL.next,
          }
        }
        renderWizard()
        render(true)
        break
      }
      if (wizard.state === 'awaitingUser' && wizard.mode === 'WebView') {
        // 用户点「我已完成」→ 通知后端进入 Verifying（后端从登录窗口取 Cookie）
        wizard.state = 'verifying'
        wizard.stepIndex = 2
        renderWizard()
        await authBegin(wizard.scope, 'VerifyDone').catch(() => {})
        break
      }
      if (wizard.state === 'blocked') {
        wizard.mode = 'WebView'
        renderWizard()
      }
      await withBusy(button, '处理中…', async () => {
        try {
          const ticket = await authBegin(wizard.scope, wizard.mode)
          wizard.ticket = ticket?.id || ''
          startWizardPolling()
          if (wizard.mode === 'Password') {
            wizard.state = 'launching'
            wizard.stepIndex = 1
          } else if (wizard.mode === 'WebView') {
            wizard.state = 'launching'
            wizard.stepIndex = 0
          } else {
            wizard.state = 'awaitingUser'
            wizard.stepIndex = 1
          }
        } catch (error) {
          wizard.state = 'failed'
          const code = 'AUTH_E_' + String(error?.code || 'INTERNAL').toUpperCase()
          const copy = AUTH_ERROR_COPY[code] || AUTH_ERROR_COPY.AUTH_E_INTERNAL
          wizard.error = { code, message: String(error?.message || error), next: copy.next }
        }
      })
      renderWizard()
      break
    }
    default:
      break
  }
})

/* ------------------------------ 通知授权 ------------------------------ */

async function ensureNotifyPermission() {
  const granted = await notifySystem('测试通知', '如果你看到这条，说明桌面提醒可用。')
  if (granted) toast('ok', '已开启', '日程到点会在系统通知里提醒你')
  else toast('error', '未授权', '请在系统设置里允许本应用发送通知；界面内提醒仍然有效')
}

/* ------------------------------ 登录向导（§5.2：11 状态 + 3 步指示） ------------------------------ */

const WIZARD_STATES = {
  unconfigured: { title: '还没登录学校门户', hint: '登录后才会抓取到通知', action: '去登录', cancellable: true },
  ready: { title: '准备登录', hint: '选择登录方式（窗口登录 / 账号密码）', action: '开始', cancellable: true },
  launching: { title: '正在打开登录页', hint: '若 5 秒未加载，可改用粘贴 Cookie', action: '取消', cancellable: true },
  awaitingUser: { title: '请在窗口中完成登录', hint: '需要验证码/短信时也在这里完成', action: '我已完成', cancellable: true },
  verifying: { title: '正在确认登录状态', hint: '校验门户是否下发了会话 Cookie', action: '', cancellable: false },
  active: { title: '已登录', hint: '下次抓取将自动使用', action: '重新登录', cancellable: true },
  expiring: { title: '登录即将过期', hint: '已提前续期，无需操作', action: '立即续期', cancellable: true },
  expired: { title: '登录已过期', hint: '正在自动重登', action: '手动登录', cancellable: true },
  cancelled: { title: '已取消登录', hint: '不影响已抓到的通知', action: '重新登录', cancellable: true },
  failed: { title: '登录失败', hint: '见下方错误说明与下一步动作', action: '重试', cancellable: true },
  blocked: { title: '需要人工验证', hint: '出现验证码，自动登录不可用', action: '改用窗口登录', cancellable: true },
}

/** §5.3 错误码 → 文案 + 下一步动作（与后端 AuthErrorCode 对齐）。 */
const AUTH_ERROR_COPY = {
  AUTH_E_NETWORK: { message: '无法连接学校门户', next: '检查是否在校内网；一键开启 WebVPN' },
  AUTH_E_TLS: { message: '证书不受信任', next: '展示指纹，需显式信任（仅该源）' },
  AUTH_E_CREDENTIAL: { message: '账号或密码不对', next: '重填；连续 3 次转 WebView' },
  AUTH_E_CAPTCHA: { message: '需要验证码', next: '转 WebView 登录' },
  AUTH_E_MFA: { message: '需要二次验证', next: '转 WebView 登录' },
  AUTH_E_FORM: { message: '登录页结构变了', next: '转 WebView；附诊断信息' },
  AUTH_E_NO_PORTAL_SESSION: { message: '登录成功但门户没给会话', next: '自动尝试 2 个候选回调地址，再引导改 service' },
  AUTH_E_GATEWAY: { message: 'WebVPN 会话过期', next: '重新登录网关，更新网关 Cookie' },
  AUTH_E_PORTAL_BIZ: { message: '门户登录态失效', next: '触发续期；失败转 WebView' },
  AUTH_E_CANCELLED: { message: '已取消', next: '可重试' },
  AUTH_E_INTERNAL: { message: '未知错误', next: '导出诊断包' },
}

const wizard = {
  open: false,
  state: 'ready',
  scope: 'portal',
  mode: 'WebView',
  stepIndex: 0,
  ticket: '',
  error: null, // { code, message, next }
}

function renderWizard() {
  const overlay = $('#login-wizard')
  if (!overlay) return
  overlay.hidden = !wizard.open
  if (!wizard.open) return
  const spec = WIZARD_STATES[wizard.state] || WIZARD_STATES.ready
  $('#wizard-title').textContent = spec.title
  $('#wizard-hint').textContent = spec.hint
  const steps = ['打开登录页', '完成验证', '确认会话']
  $('#wizard-steps').innerHTML = steps
    .map((label, index) => `<span class="wizard-step ${index < wizard.stepIndex ? 'done' : index === wizard.stepIndex ? 'current' : ''}">${index + 1}. ${esc(label)}</span>`)
    .join('<i class="wizard-arrow">→</i>')
  const errorBox = $('#wizard-error')
  if (wizard.error) {
    errorBox.hidden = false
    errorBox.innerHTML = `<b>${esc(wizard.error.code)}</b> ${esc(wizard.error.message)}<br>下一步：${esc(wizard.error.next)}`
  } else {
    errorBox.hidden = true
  }
  $('#wizard-modes').innerHTML = ['WebView', 'Password', 'ImportCookie']
    .map(
      (mode) =>
        `<button class="btn small ${wizard.mode === mode ? 'primary' : 'ghost'}" data-act="wizard-mode" data-mode="${mode}">${
          mode === 'WebView' ? '窗口登录（推荐）' : mode === 'Password' ? '账号密码' : '粘贴 Cookie'
        }</button>`,
    )
    .join('')
  const primary = $('#wizard-primary')
  primary.textContent = spec.action || ''
  primary.hidden = !spec.action
  const cancel = overlay.querySelector('[data-act="wizard-cancel"]')
  cancel.hidden = !spec.cancellable
  // ImportCookie 模式：给粘贴入口
  if (wizard.mode === 'ImportCookie' && !$('#wizard-cookie')) {
    $('#wizard-modes').insertAdjacentHTML(
      'afterend',
      `<label class="field" id="wizard-cookie"><span>粘贴登录态 Cookie</span>
        <textarea id="wizard-cookie-text" placeholder="浏览器 F12 → Network → 复制整条 Cookie"></textarea></label>`,
    )
  } else if (wizard.mode !== 'ImportCookie' && $('#wizard-cookie')) {
    $('#wizard-cookie').remove()
  }
}

function openWizard() {
  wizard.open = true
  wizard.error = null
  wizard.state = state?.meta?.account?.configured ? 'ready' : 'ready'
  renderWizard()
}

function closeWizard() {
  wizard.open = false
  renderWizard()
}

function applyAuthProgress(payload) {
  if (!payload) return
  // payload.state 为 {state: 'launching'|...} 或 {state:'failed', kind:...}
  const s = payload.state || {}
  const raw = s.state || ''
  if (raw === 'failed') {
    wizard.state = 'failed'
    const code = s.kind?.code || s.kind || 'AUTH_E_INTERNAL'
    const copy = AUTH_ERROR_COPY[code] || AUTH_ERROR_COPY.AUTH_E_INTERNAL
    wizard.error = { code, message: copy.message, next: copy.next }
  } else {
    wizard.state = raw
    wizard.error = null
  }
  wizard.stepIndex = Number(payload.step_index) || 0
  if (payload.hint) WIZARD_STATES[wizard.state] && (WIZARD_STATES[wizard.state].hint = payload.hint)
  renderWizard()
}

/* 500ms 轮询兜底（登录期间事件可能丢失） */
let wizardPollTimer = null
function startWizardPolling() {
  if (wizardPollTimer) return
  wizardPollTimer = setInterval(async () => {
    if (!wizard.open) return
    // 用 get_state 的 account.lastLogin 作为弱状态源兜底
    try {
      await fetchState()
      render()
    } catch {
      /* 忽略 */
    }
  }, 500)
}
function stopWizardPolling() {
  if (wizardPollTimer) clearInterval(wizardPollTimer)
  wizardPollTimer = null
}

/* ------------------------------ 抓取范围浮窗 ------------------------------ */

/** 打开抓取浮窗：用设置页默认值回填，临时范围不写回设置。 */
function openPollRange() {
  const dialog = $('#poll-range-dialog')
  if (!dialog) return
  const settings = state?.settings || {}
  const startEl = $('#poll-range-start')
  const endEl = $('#poll-range-end')
  if (startEl) startEl.value = settings.dateRangeStart || ''
  if (endEl) endEl.value = settings.dateRangeEnd || ''
  syncRangePresetState('poll')
  dialog.hidden = false
}

function closePollRange() {
  const dialog = $('#poll-range-dialog')
  if (dialog) dialog.hidden = true
}

/* ------------------------------ 启动 ------------------------------ */

$('#btn-poll').addEventListener('click', openPollRange)
$('#btn-notify').addEventListener('click', ensureNotifyPermission)

/* 手改浮窗日期即「自定义」：静态节点，只在启动时各绑一次，openPollRange 里不再绑定 */
for (const rangeInputId of ['poll-range-start', 'poll-range-end']) {
  $(`#${rangeInputId}`)?.addEventListener('change', () => syncRangePresetState('poll'))
}

/* 外链统一走 opener（S5 白名单），顺带记已读 */
document.addEventListener('click', (event) => {
  const link = event.target.closest('a[target="_blank"]')
  if (!link) return
  event.preventDefault()
  openExternal(link.href)
  const id = link.dataset.id
  if (id) api('notice.read', { id, read: true }, { quiet: true })
})

async function boot() {
  // 首屏骨架：读取 state 前避免白屏（数据到达后由 render 覆盖）
  renderSkeleton()
  try {
    await fetchState()
  } catch {
    $('#status-line').textContent = '连不上本地内核，尝试重启应用'
    return
  }
  handleReminders()
  render(true)

  /* 事件订阅：5 通道 */
  listenTauri('state://changed', (payload) => {
    if (payload?.state) {
      applyState(payload.state, payload.version)
      handleReminders()
      render()
    }
  })
  listenTauri('poll://progress', (payload) => {
    const bar = $('#poll-progress')
    if (!bar) return
    if (payload && payload.phase !== 'done') {
      bar.hidden = false
      const pct = payload.total ? Math.round((payload.done / payload.total) * 100) : 30
      bar.querySelector('i').style.width = `${pct}%`
      if (!attachmentProgress) {
        $('#status-line').textContent = `正在抓取 ${payload.sourceId || ''}（${payload.done}/${payload.total}）`
      }
    } else {
      bar.hidden = true
    }
  })
  listenTauri('attachment://progress', (payload) => {
    const bar = $('#poll-progress')
    if (!payload || payload.phase === 'done') {
      attachmentProgress = null
      if (bar) bar.hidden = true
      render(true)
      return
    }
    attachmentProgress = {
      done: Number(payload.done) || 0,
      total: Number(payload.total) || 0,
      fileName: String(payload.fileName || ''),
    }
    $('#status-line').textContent = `正在下载附件 ${attachmentProgress.done}/${attachmentProgress.total}`
    if (bar) {
      bar.hidden = false
      const total = attachmentProgress.total
      bar.querySelector('i').style.width = `${total ? Math.round((attachmentProgress.done / total) * 100) : 30}%`
    }
  })
  listenTauri('auth://progress', (payload) => {
    applyAuthProgress(payload)
  })
  listenTauri('reminder://fired', () => {
    handleReminders()
    render()
  })
  listenTauri('log://line', (payload) => {
    if (payload?.level === 'warn' || payload?.level === 'error') {
      console.warn('[log]', payload.text)
    }
  })

  /* 静默刷新兜底：事件之外 10s 轮询（与 Web 版一致） */
  setInterval(async () => {
    if (document.visibilityState !== 'visible') return
    try {
      await fetchState()
      handleReminders()
      render()
    } catch {
      /* 网络抖动忽略 */
    }
  }, 10000)
}

boot()
