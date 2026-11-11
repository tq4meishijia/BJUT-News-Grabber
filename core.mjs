/**
 * 校园通知核心：把网页变成「事件」的纯逻辑。
 * 抓取（支持 DNS 覆盖 / gzip / GBK）、三种列表解析、正文定位、截止时间与联系人抽取、打分、规则摘要。
 * 不依赖任何第三方包，也不碰文件系统 —— 便于单独测试。
 */
import { createHash, createCipheriv, createDecipheriv } from 'node:crypto'
import http from 'node:http'
import https from 'node:https'
import dns from 'node:dns'
import zlib from 'node:zlib'
const DEFAULT_KEYWORDS = [
  '四级', '六级', '四六级', '选课', '补选', '退课', '重选', '考试', '补考', '重修', '缓考',
  '报名', '截止', '讲座', '报告', '成绩', '绩点', '学分', '奖学金', '助学金', '助学贷款',
  '竞赛', '大赛', '实习', '招聘', '宣讲', '保研', '推免', '论文', '答辩', '毕业', '调课',
  '停课', '放假', '假期', '校历', '缴费', '证书', '体测', '体检', '评教', '志愿', '交换',
  '国际交流', '学籍', '转专业', '辅修', '双学位', '宿舍', '校园卡', '图书馆', '实验室',
]

const NOTICE_WORDS = /通知|公告|公示|通告|讲座|报告|报名|考试|评选|申报|征集|安排|意见|招标|中标|招聘|宣讲|学术/i

const NOISE_TITLES = new Set([
  '更多', '查看更多', '首页', '上一页', '下一页', '尾页', '返回', '详情', '登录', '注册',
  '网站首页', '学校主页', '联系我们', 'English', 'EN', '中文',
])

const DEFAULT_SOURCES = [
  {
    id: 'bjut-portal-1',
    name: '一站式 · 校发通知',
    type: 'json',
    url: 'https://my.bjut.edu.cn/site/get_news_list?category_id=1&p=1&size=32',
    pageUrl: 'https://my.bjut.edu.cn/site/get_news_list?category_id=1&p={n}&size=32',
    pages: 2,
    enabled: true,
    itemsPath: 'd.data',
    map: { title: 'title', id: 'id', date: 'createdate', summary: 'summary' },
    detailUrlTemplate: 'https://my.bjut.edu.cn/site/get_news_detail?id={id}',
    contentPath: 'd.content',
    note: '学校层面的发文（category_id=1）。首次使用需要登录一次：设置页填账号密码，或在数据源里贴 Cookie。',
  },
  {
    id: 'bjut-portal-3',
    name: '一站式 · 公示公告',
    type: 'json',
    url: 'https://my.bjut.edu.cn/site/get_news_list?category_id=3&p=1&size=32',
    pageUrl: 'https://my.bjut.edu.cn/site/get_news_list?category_id=3&p={n}&size=32',
    pages: 2,
    enabled: true,
    itemsPath: 'd.data',
    map: { title: 'title', id: 'id', date: 'createdate', summary: 'summary' },
    detailUrlTemplate: 'https://my.bjut.edu.cn/site/get_news_detail?id={id}',
    contentPath: 'd.content',
    note: '评奖评优、名单公示等（category_id=3）。',
  },
  {
    id: 'bjut-portal-4',
    name: '一站式 · 教学通告',
    type: 'json',
    url: 'https://my.bjut.edu.cn/site/get_news_list?category_id=4&p=1&size=32',
    pageUrl: 'https://my.bjut.edu.cn/site/get_news_list?category_id=4&p={n}&size=32',
    pages: 2,
    enabled: true,
    itemsPath: 'd.data',
    map: { title: 'title', id: 'id', date: 'createdate', summary: 'summary' },
    detailUrlTemplate: 'https://my.bjut.edu.cn/site/get_news_detail?id={id}',
    contentPath: 'd.content',
    note: '教务处 / 研究生院 / 都柏林学院的教学类通知（category_id=4）。',
  },
  {
    id: 'bjut-portal-6',
    name: '一站式 · 院部处通知',
    type: 'json',
    url: 'https://my.bjut.edu.cn/site/get_news_list?category_id=6&p=1&size=32',
    pageUrl: 'https://my.bjut.edu.cn/site/get_news_list?category_id=6&p={n}&size=32',
    pages: 3,
    enabled: true,
    itemsPath: 'd.data',
    map: { title: 'title', id: 'id', date: 'createdate', summary: 'summary' },
    detailUrlTemplate: 'https://my.bjut.edu.cn/site/get_news_detail?id={id}',
    contentPath: 'd.content',
    note: '各院部处的通知，量最大（category_id=6）。',
  },
]

/* ------------------------------------------------------------------ */
/* 校外模式（WebVPN）：出站地址统一重写，响应里的链接统一还原             */
/* ------------------------------------------------------------------ */

/** BJUT 专用：WebVPN 网关内置，界面与配置里都不需要填地址。换网关改这一行。 */
const WEBVPN_GATEWAY = 'https://webvpn.bjut.edu.cn'

/** Wengine 网关用固定密钥/IV 对主机名做 AES-128-CFB，IV 明文拼在密文前。 */
const WENGINE_KEY = 'wrdvpnisthebest!'

/** 默认「网关会话过期」登录页特征：只认通用登录页结构，不写死某校文案。 */
const DEFAULT_LOGIN_PAGE_PATTERN =
  `<input[^>]+type=["']?password|<title[^>]*>[^<]*(?:登录|登陆|login|身份认证)[^<]*</title>|id=["']?(?:unPassword|pwPassword|cptPassword|loginForm)`

/** 校外模式默认配置：默认关闭；网关地址固定，用户只需开关与登录态 Cookie。 */
const DEFAULT_WEBVPN = {
  enabled: false,
  gateway: WEBVPN_GATEWAY,
  cookie: '',
  loginPagePattern: DEFAULT_LOGIN_PAGE_PATTERN,
}

function emptyStore(config) {
  return {
    version: 1,
    settings: {
      pollMinutes: numberOr(config?.pollMinutes, 30, 5, 1440),
      keywords: Array.isArray(config?.keywords) && config.keywords.length
        ? config.keywords.slice()
        : DEFAULT_KEYWORDS.slice(),
      llm: config?.llm !== false,
      llmMaxPerPoll: numberOr(config?.llmMaxPerPoll, 20, 1, 40),
      autoSchedule: config?.autoSchedule !== false,
      autoRemindBefore: Array.isArray(config?.autoRemindBefore) && config.autoRemindBefore.length
        ? config.autoRemindBefore.map((n) => numberOr(n, 60, 0, 100000))
        : [1440, 120],
      maxItemsPerSource: numberOr(config?.maxItemsPerSource, 100, 1, 200),
      fetchDetail: config?.fetchDetail !== false,
      detailLimit: numberOr(config?.detailLimit, 25, 5, 60),
      notify: true,
      webvpn: { ...DEFAULT_WEBVPN },
    },
    sources: DEFAULT_SOURCES.map((s) => ({
      enabled: false,
      type: 'html',
      cookie: '',
      resolveIp: '',
      linkPattern: '',
      pages: 1,
      pageUrl: '',
      urlTemplate: '',
      detailUrlTemplate: '',
      contentPath: '',
      itemsPath: '',
      map: null,
      status: 'idle',
      statusText: '',
      lastFetchAt: '',
      lastCount: 0,
      ...s,
    })),
    notices: [],
    items: [],
    meta: { lastPollAt: '', lastPollError: '', pollCount: 0 },
  }
}

function numberOr(value, fallback, min, max) {
  const n = Number(value)
  if (!Number.isFinite(n)) return fallback
  return Math.min(max, Math.max(min, Math.round(n)))
}

/* ------------------------------------------------------------------ */
/* 文本工具                                                             */
/* ------------------------------------------------------------------ */

function sha1(text) {
  return createHash('sha1').update(text).digest('hex')
}

function stripTags(html) {
  return String(html || '')
    .replace(/<script[\s\S]*?<\/script>/gi, ' ')
    .replace(/<style[\s\S]*?<\/style>/gi, ' ')
    .replace(/<br\s*\/?>/gi, ' ')
    .replace(/<\/(p|div|li|tr|h\d)>/gi, ' ')
    .replace(/<[^>]+>/g, ' ')
}

function decodeEntities(text) {
  const named = {
    '&nbsp;': ' ', '&amp;': '&', '&lt;': '<', '&gt;': '>', '&quot;': '"', '&#39;': "'",
    '&ldquo;': '“', '&rdquo;': '”', '&mdash;': '—', '&hellip;': '…', '&middot;': '·',
  }
  let out = String(text || '')
  for (const [key, value] of Object.entries(named)) out = out.split(key).join(value)
  out = out.replace(/&#x([0-9a-f]+);/gi, (_, hex) => String.fromCodePoint(parseInt(hex, 16)))
  out = out.replace(/&#(\d+);/g, (_, dec) => String.fromCodePoint(Number(dec)))
  return out
}

function cleanText(html) {
  return decodeEntities(stripTags(html))
    /* 学校 CMS 常见的编辑器残渣，留着会污染摘要第一句 */
    .replace(/窗体顶端|窗体底端|上一条：|下一条：/g, ' ')
    .replace(/[\u200b\ufeff]/g, '')
    .replace(/[\s\u00a0\u3000]+/g, ' ')
    .trim()
}

/** 去掉列表里常见的栏目前缀与日期前缀，让标题更像「一件事」。 */
function cleanTitle(raw) {
  let text = cleanText(raw)
  text = text.replace(/^【[^】]{1,14}】\s*/, '')
  text = text.replace(/^\[[^\]]{1,14}\]\s*/, '')
  text = text.replace(/^\d{1,2}\s+\d{4}[.\-/]\d{1,2}\s+/, '')
  text = text.replace(/^\d{4}\s*[.\-/年]\s*\d{1,2}\s*[.\-/月]\s*\d{1,2}\s*日?\s+/, '')
  return text.trim()
}

function pad2(n) {
  return String(n).padStart(2, '0')
}

function isoDate(y, m, d) {
  if (!(y >= 2000 && y <= 2100 && m >= 1 && m <= 12 && d >= 1 && d <= 31)) return ''
  return `${y}-${pad2(m)}-${pad2(d)}`
}

/** 从一段文字里找出最像「发布日期」的那个日期，返回 YYYY-MM-DD。 */
function findDate(text) {
  const s = String(text || '')
  let m = /(\d{4})\s*[-/年.]\s*(\d{1,2})\s*[-/月.]\s*(\d{1,2})\s*日?/.exec(s)
  if (m) {
    const out = isoDate(Number(m[1]), Number(m[2]), Number(m[3]))
    if (out) return out
  }
  // 「22 2026.10 标题」：日 + 年月
  m = /(?:^|\s)(\d{1,2})\s+(\d{4})[.](\d{1,2})(?![\d.])/.exec(s)
  if (m) {
    const out = isoDate(Number(m[2]), Number(m[3]), Number(m[1]))
    if (out) return out
  }
  m = /(\d{4})\s*[-/年.]\s*(\d{1,2})\s*月?(?![\d-/.])/.exec(s)
  if (m) {
    const out = isoDate(Number(m[1]), Number(m[2]), 1)
    if (out) return out
  }
  m = /(\d{1,2})\s*[-/月]\s*(\d{1,2})\s*日?(?![\d-/.])/.exec(s)
  if (m) {
    const out = isoDate(new Date().getFullYear(), Number(m[1]), Number(m[2]))
    if (out) return out
  }
  return ''
}

/** 通知正文里的截止时间：「截止」「请于X月X日前」「截至」等。 */
function findDeadline(text) {
  const s = String(text || '')
  // 先走几条高置信度的固定句式（参考成熟爬虫的写法，误报更低）
  const strict = [
    /(?:截止|截至)[^。；;\n]{0,14}?(\d{4})\s*[年./-]\s*(\d{1,2})\s*[月./-]\s*(\d{1,2})\s*日?/,
    /(?:于|在)\s*(\d{4})\s*[年./-]\s*(\d{1,2})\s*[月./-]\s*(\d{1,2})\s*日?\s*(?:前|之前|以前|止|截止)/,
    /(\d{4})\s*[年./-]\s*(\d{1,2})\s*[月./-]\s*(\d{1,2})\s*日?\s*(?:前|之前|以前|截止)/,
    /(\d{1,2})\s*月\s*(\d{1,2})\s*日\s*(?:前|之前|以前|截止)/,
  ]
  for (const pattern of strict) {
    const hit = pattern.exec(s)
    if (!hit) continue
    const year = hit.length === 4 ? Number(hit[1]) : new Date().getFullYear()
    const month = Number(hit.length === 4 ? hit[2] : hit[1])
    const day = Number(hit.length === 4 ? hit[3] : hit[2])
    const date = isoDate(year, month, day)
    if (!date) continue
    const after = s.slice(hit.index, hit.index + hit[0].length + 30)
    const clock = /(\d{1,2})\s*[:：]\s*(\d{2})/.exec(after) || /(\d{1,2})\s*[点时](?!\d)/.exec(after)
    const hm = clock
      ? `${pad2(Number(clock[1]))}:${pad2(Number(clock[2] ?? 0))}`
      : '23:59'
    return `${date}T${hm}`
  }
  const anchors = /(截止|截至|不晚于|务必于|请于|须于|需要在|前完成|前提交|前报名)/g
  let m
  while ((m = anchors.exec(s)) !== null) {
    const window = s.slice(Math.max(0, m.index - 30), Math.min(s.length, m.index + 60))
    const date = findDate(window)
    if (!date) continue
    const time = /(?:^|[^\d])(\d{1,2})\s*[:：]\s*(\d{2})/.exec(window)
    const hourOnly = /(\d{1,2})\s*[点时](?!\d)/.exec(window)
    let hm = '23:59'
    if (time) hm = `${pad2(Number(time[1]))}:${pad2(Number(time[2]))}`
    else if (hourOnly) hm = `${pad2(Number(hourOnly[1]))}:00`
    return `${date}T${hm}`
  }
  // 「11月5日前」但没有「截止」字样
  const m2 = /(\d{1,2})\s*月\s*(\d{1,2})\s*日\s*(?:之?前|截止)/.exec(s)
  if (m2) {
    const date = isoDate(new Date().getFullYear(), Number(m2[1]), Number(m2[2]))
    if (date) return `${date}T23:59`
  }
  return ''
}

function cut(text, max) {
  const s = String(text || '')
  return s.length <= max ? s : `${s.slice(0, max - 1)}…`
}

/** 从正文里找「找谁办」：联系人与电话。 */
function findContact(text) {
  const s = String(text || '')
  const nameHit = /(?:联系人|联系老师|负责老师)[：:\s]*([\u4e00-\u9fa5]{2,4}(?:老师|同学)?)/.exec(s)
  const phoneHit =
    /(?:电话|联系电话|联系方式)[：:\s]*(0\d{2,3}-?\d{7,8}|1[3-9]\d{9}|\d{8})/.exec(s) ||
    /(0\d{2,3}-?\d{7,8})/.exec(s)
  const parts = []
  if (nameHit) parts.push(nameHit[1])
  if (phoneHit) parts.push(phoneHit[1])
  return parts.join(' · ')
}

/** 尽量只留正文：先按常见 CMS 容器定位，再兜底整页。 */
function extractMainContent(html) {
  const source = String(html || '')
  const markers = [
    /id=["']?vsb_content/i,
    /class=["'][^"']*v_news_content/i,
    /class=["'][^"']*TRS_Editor/i,
    /class=["'][^"']*(?:article[-_]?content|article|news[-_]?content|content[-_]?box)[^"']*["']/i,
    /id=["']?(?:content|article|zoom)["']?/i,
  ]
  let start = -1
  for (const marker of markers) {
    const hit = marker.exec(source)
    if (hit) {
      start = hit.index
      break
    }
  }
  let slice = source
  if (start >= 0) {
    const tagEnd = source.indexOf('>', start)
    slice = source.slice(tagEnd >= 0 ? tagEnd + 1 : start, start + 40000)
  }
  const tails = [
    '上一篇', '下一篇', '分享到', '打印本页', '关闭窗口', '相关附件', '版权所有', '京公网安备',
  ]
  let end = slice.length
  for (const tail of tails) {
    const index = slice.indexOf(tail)
    if (index > 200 && index < end) end = index
  }
  slice = slice.slice(0, end)
  return htmlToText(slice).slice(0, 6000)
}

/**
 * HTML → 纯文本：块级标签换行，行内标签直接去掉。
 * 行内标签（span/font/b…）如果也换成空格，字数统计类页面会把「2026」拆成「202 6」。
 */
function htmlToText(html) {
  const body = String(html || '')
    .replace(/<script[\s\S]*?<\/script>/gi, ' ')
    .replace(/<style[\s\S]*?<\/style>/gi, ' ')
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<\/(p|div|li|tr|h\d|td|section|article)>/gi, '\n')
    .replace(/<\/?(span|font|b|strong|em|i|a|u|sub|sup|label|small|big|abbr|cite)\b[^>]*>/gi, '')
  return cleanText(body)
}

function pickTags(text, keywords) {
  const hit = []
  for (const word of keywords) {
    if (word && text.includes(word) && !hit.includes(word)) hit.push(word)
    if (hit.length >= 6) break
  }
  return hit
}

function daysUntil(deadlineIso) {
  if (!deadlineIso) return null
  const due = new Date(deadlineIso).getTime()
  if (!Number.isFinite(due)) return null
  return (due - Date.now()) / 86400000
}

function scoreNotice({ title, content, tags, deadline, keywords }) {
  let score = 22
  const body = `${title} ${content || ''}`
  for (const tag of tags) {
    if (body.includes(tag)) score += 6
  }
  const days = daysUntil(deadline)
  if (days !== null) {
    if (days < 0) score += 2
    else if (days <= 3) score += 30
    else if (days <= 7) score += 24
    else if (days <= 30) score += 12
  }
  if (/紧急|务必|立即|马上|今天|今日|明天/.test(body)) score += 8
  if (/讲座|报告会|学术报告|宣讲/.test(body)) score += 4
  if (/成绩|绩点|学分|毕业|学位|学籍/.test(body)) score += 8
  if (/招标|中标|采购|物业|食堂改造/.test(title)) score -= 18
  if (/下载专区|规章制度|资料汇编/.test(title)) score -= 10
  return Math.max(5, Math.min(98, Math.round(score)))
}

/** 去掉红头文件开头的「XX大学文件 工大发〔2022〕15号 ——————」页眉块。 */
function stripRedHeader(text) {
  const source = String(text || '')
  const header = /^[\s\S]{0,160}?[—\-_=]{16,}/.exec(source)
  return header ? source.slice(header[0].length).trim() : source
}

/** 模型不可用时的规则式摘要，保证「对我有什么用」永远有内容。 */
function ruleSummary({ title, content, tags, deadline, contact }) {
  /* 红头文件开头常是「XX大学文件 工大发〔2022〕15号 ——————」，摘第一句前先剥掉 */
  const source = stripRedHeader(content)
  /* 正文常常以标题开头，摘第一句时先把重复的标题去掉 */
  const flatTitle = String(title || '').replace(/\s+/g, '')
  const flatBody = String(source || '').replace(/\s+/g, ' ').replace(/\s+/g, ' ')
  let body = source
  if (flatTitle.length >= 8 && flatBody.replace(/\s+/g, '').startsWith(flatTitle)) {
    body = flatBody.replace(/\s+/g, '').slice(flatTitle.length)
  }
  const bodyForMatch = `${title} ${body}`
  const useful = []
  if (tags.length) useful.push(`关联：${tags.join('、')}`)
  if (deadline) {
    const days = daysUntil(deadline)
    const when = deadline.replace('T', ' ')
    if (days !== null && days >= 0) useful.push(`截止：${when}（还有 ${Math.max(0, Math.ceil(days))} 天）`)
    else if (days !== null) useful.push(`截止时间已过：${when}`)
    else useful.push(`截止：${when}`)
  }
  if (contact) useful.push(`找谁办：${contact}`)
  if (/报名|申请|提交|上传|填报|填写/.test(bodyForMatch)) useful.push('需要你主动报名/提交材料')
  if (/考试|补考|重修|缓考|选课/.test(bodyForMatch)) useful.push('和考试/选课安排相关，建议进系统确认')
  if (/讲座|报告会|学术报告/.test(bodyForMatch)) useful.push('讲座类，注意时间地点是否需要报名')
  if (/评选|奖学金|助学金|申请/.test(bodyForMatch)) useful.push('涉及评选或资助，留意资格条件')
  if (!useful.length) useful.push('校园事务类，快速浏览即可')
  const firstSentence = String(body || '')
    .split(/[。！？\n]/)
    .map((s) => s.trim())
    .find((s) => s.length >= 12)
  return {
    summary: cut(firstSentence || title, 110),
    useful: useful.slice(0, 4),
  }
}

/* ------------------------------------------------------------------ */
/* 校外模式：URL 重写 / 还原（纯函数，互为逆运算，可单独断言）             */
/* ------------------------------------------------------------------ */

/** 网关地址内置，只需开关打开即激活，无需任何配置。 */
function isWebvpnEnabled(cfg) {
  return Boolean(cfg && cfg.enabled === true)
}

/** 网关地址固定内置：忽略配置里可能残留的旧值。 */
function gatewayOf() {
  return WEBVPN_GATEWAY.replace(/\/+$/, '')
}

function escapeRegExp(text) {
  return String(text).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/* Wengine 主机名编解码：IV 十六进制 + AES-128-CFB 密文（只取主机名长度）的十六进制。
   算法已用公开样例 www.baidu.com 与 BJUT 真实网关响应双向核对。 */
const WENGINE_GATEWAY_HOST = new URL(WEBVPN_GATEWAY).host

function encodeWengineHost(host) {
  const key = Buffer.from(WENGINE_KEY, 'utf8')
  const plain = Buffer.from(host, 'utf8')
  const cipher = createCipheriv('aes-128-cfb', key, key)
  const secret = Buffer.concat([cipher.update(plain), cipher.final()])
  return key.toString('hex') + secret.subarray(0, plain.length).toString('hex')
}

function decodeWengineHost(hex) {
  if (!/^[0-9a-fA-F]+$/.test(hex) || hex.length < 34 || hex.length % 2) return ''
  try {
    const key = Buffer.from(hex.slice(0, 32), 'hex')
    if (key.length !== 16) return ''
    const secret = Buffer.from(hex.slice(32), 'hex')
    const decipher = createDecipheriv('aes-128-cfb', key, key)
    return Buffer.concat([decipher.update(secret), decipher.final()]).toString('utf8')
  } catch {
    return ''
  }
}

/** 网关改写后的地址可能是绝对网址，也可能是 /https/<编码>/… 形式的相对路径。 */
function wenginePattern(anchored) {
  const source =
    (anchored ? '^' : '') +
    '(?:https?://' + escapeRegExp(WENGINE_GATEWAY_HOST) + ')?' +
    '/(https?(?:-\\d+)?)/([0-9a-fA-F]{34,})' +
    '(/[^"\'<>\\s]*)?' +
    (anchored ? '$' : '')
  return new RegExp(source, anchored ? 'i' : 'gi')
}
const WENGINE_ABS = wenginePattern(true)
const WENGINE_ALL = wenginePattern(false)

function restoreWengineMatch(all, scheme, hex, path) {
  const host = decodeWengineHost(hex)
  if (!host) return all
  const port = /-(\d+)$/.exec(scheme)
  const protocol = scheme.startsWith('https') ? 'https' : 'http'
  return protocol + '://' + host + (port ? ':' + port[1] : '') + (path || '/')
}

/**
 * 出站重写：门户原始网址 → Wengine 网关地址。
 * 关闭（或已经是网关地址）时原样返回，直连路径逐字节不变。
 */
function rewriteUrl(url, cfg) {
  if (!isWebvpnEnabled(cfg)) return url
  const value = String(url || '')
  const gateway = gatewayOf()
  if (!/^https?:\/\//i.test(value)) return value
  /* 网关回的 Location、已改写过的地址不再二次包裹 */
  if (value.toLowerCase().startsWith(gateway.toLowerCase())) return value
  let target
  try {
    target = new URL(value)
  } catch {
    return value
  }
  const protocol = target.protocol.slice(0, -1)
  const nonStandardPort = target.port &&
    !((protocol === 'https' && target.port === '443') || (protocol === 'http' && target.port === '80'))
  const scheme = protocol + (nonStandardPort ? '-' + target.port : '')
  return gateway + '/' + scheme + '/' + encodeWengineHost(target.hostname) + target.pathname + target.search
}

/** 还原单个地址：网关地址 → 门户原始网址（与 rewriteUrl 互为逆运算）。 */
function restoreUrl(url, cfg) {
  if (!isWebvpnEnabled(cfg)) return url
  const value = String(url || '')
  const hit = WENGINE_ABS.exec(value)
  return hit ? restoreWengineMatch(hit[0], hit[1], hit[2], hit[3]) : value
}

/**
 * 还原响应文本里被网关改写过的链接，让 new URL / 去重 / 详情跟进
 * 仍按门户原始地址走。未启用时直接返回原参数。
 */
function restoreUrlsInText(text, cfg) {
  if (!isWebvpnEnabled(cfg)) return text
  const source = String(text ?? '')
  if (!source) return source
  return source.replace(WENGINE_ALL, restoreWengineMatch)
}

/* ---------------------- 网关会话过期的响应识别 ---------------------- */

/** 门户正常业务响应里常见的数据容器：有非空值就说明不是过期页。 */
const PORTAL_DATA_KEYS = ['d', 'data', 'items', 'list', 'rows', 'records', 'result', 'content']

/** 网关 JSON 错误体里的登录 / 会话语义（内置登录页特征之外的一层兜底）。 */
const SESSION_WORDS = /请登录|未登录|登录超时|重新登录|session expired|login required|unauthorized|请先登录/i

function matchesLoginPattern(text, cfg) {
  const pattern = String(cfg?.loginPagePattern || DEFAULT_LOGIN_PAGE_PATTERN).trim()
  try {
    return new RegExp(pattern, 'i').test(text)
  } catch {
    return false
  }
}

/** 含非空业务数据容器的 JSON 一律当正常数据，绝不当成过期页。 */
function hasPortalData(value) {
  if (!value || typeof value !== 'object') return false
  if (Array.isArray(value)) return value.length > 0
  for (const key of PORTAL_DATA_KEYS) {
    const nested = value[key]
    if (Array.isArray(nested) && nested.length) return true
    if (nested && typeof nested === 'object' && Object.keys(nested).length) return true
  }
  return false
}

/** 已排除正常业务数据的 JSON：按登录语义或 401/403/302 + 消息判定。 */
function jsonSessionExpired(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const texts = []
  let flaggedCode = false
  let hasMessage = false
  for (const [key, val] of Object.entries(value)) {
    const lower = key.toLowerCase()
    if (lower === 'msg' || lower === 'message' || lower === 'm') hasMessage = true
    if (/^(?:code|e|status)$/i.test(key) && [401, 403, 302].includes(Number(val))) flaggedCode = true
    if (typeof val === 'string' || typeof val === 'number') texts.push(String(val))
    else if (val && typeof val === 'object' && !Array.isArray(val)) {
      for (const inner of Object.values(val)) {
        if (typeof inner === 'string' || typeof inner === 'number') texts.push(String(inner))
      }
    }
  }
  return SESSION_WORDS.test(texts.join(' ')) || Boolean(flaggedCode && hasMessage)
}

/**
 * 网关会话过期常返回 200 + 登录页（HTML），也可能声称 JSON 却回登录页或 JSON 错误体。
 * 对所有 content-type 生效，但用结构特征避免误伤正常业务 JSON。
 */
function webvpnSessionExpired(text, cfg) {
  if (!isWebvpnEnabled(cfg)) return false
  const raw = String(text ?? '')
  if (!raw) return false
  let parsed = null
  let isJson = true
  try {
    parsed = JSON.parse(raw)
  } catch {
    isJson = false
  }
  if (!isJson) return matchesLoginPattern(raw, cfg)
  /* 正常业务 JSON 与门户自己的业务码（e=10042/10041）交给原有分支，不抢判 */
  if (hasPortalData(parsed)) return false
  if (parsed && typeof parsed === 'object' && !Array.isArray(parsed) &&
      [10041, 10042].includes(Number(parsed.e))) return false
  return matchesLoginPattern(raw, cfg) || jsonSessionExpired(parsed)
}

/* ------------------------------------------------------------------ */
/* 网络：支持 DNS 覆盖（校内/WebVPN 域名常常公网解析不到）                  */
/* ------------------------------------------------------------------ */

async function resolveAddress(host, explicitIp) {
  if (explicitIp) return explicitIp
  try {
    const list = await dns.promises.lookup(host, { all: true })
    const v4 = list.find((entry) => entry.family === 4)
    if (v4) return v4.address
    if (list.length) return list[0].address
  } catch {
    /* 走 DoH 兜底 */
  }
  try {
    const answer = await fetchRaw(
      `https://223.5.5.5/resolve?name=${encodeURIComponent(host)}&type=1`,
      { timeoutMs: 8000 },
    )
    const parsed = JSON.parse(answer.body.toString('utf8'))
    const hit = (parsed.Answer || []).find((entry) => entry.type === 1)
    if (hit?.data) return hit.data
  } catch {
    /* 彻底解析不了 */
  }
  return ''
}

function decompress(buffer, encoding) {
  const enc = String(encoding || '').toLowerCase()
  try {
    if (enc.includes('br')) return zlib.brotliDecompressSync(buffer)
    if (enc.includes('gzip')) return zlib.gunzipSync(buffer)
    if (enc.includes('deflate')) return zlib.inflateSync(buffer)
  } catch {
    return buffer
  }
  return buffer
}

function fetchRaw(url, options = {}, redirects = 0) {
  return new Promise((resolve, reject) => {
    const vpn = isWebvpnEnabled(options.webvpn) ? options.webvpn : null
    /* 出站地址统一先过重写；网关地址（如重定向目标）不再二次包裹 */
    const outgoing = vpn ? rewriteUrl(url, vpn) : url
    let target
    try {
      target = new URL(outgoing)
    } catch {
      reject(new Error(`网址不合法：${url}`))
      return
    }
    const isHttps = target.protocol === 'https:'
    const mod = isHttps ? https : http
    const host = target.hostname
    const headers = {
      host: target.host,
      'user-agent':
        options.userAgent ||
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36',
      accept: 'text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8',
      'accept-language': 'zh-CN,zh;q=0.9,en;q=0.8',
      'accept-encoding': 'gzip, deflate',
      ...(options.headers || {}),
    }
    if (options.cookie) headers.cookie = options.cookie
    /* 网关自己那层会话 Cookie 单独附加，不混进数据源的门户 Cookie */
    if (vpn && vpn.cookie) {
      const base = String(headers.cookie || '').trim()
      headers.cookie = [base, String(vpn.cookie).trim()].filter(Boolean).join('; ')
    }
    const payload = options.body === undefined || options.body === null ? '' : String(options.body)
    if (payload && !headers['content-length']) {
      headers['content-length'] = Buffer.byteLength(payload)
    }
    const requestOptions = {
      host,
      port: target.port || (isHttps ? 443 : 80),
      path: `${target.pathname}${target.search}`,
      method: options.method || 'GET',
      headers,
      timeout: options.timeoutMs || 20000,
    }
    if (isHttps) requestOptions.servername = host
    if (options.resolveIp) {
      requestOptions.host = options.resolveIp
      requestOptions.lookup = ((_hostname, _opts, callback) => {
        callback(null, options.resolveIp, 4)
      })
    }
    const req = mod.request(requestOptions, (res) => {
      const status = res.statusCode || 0
      const location = res.headers.location
      if (!options.noFollow && status >= 300 && status < 400 && location && redirects < 4) {
        res.resume()
        let next = new URL(location, target).href
        /* 网关回的 Location 也是改写过的，先还原成门户原始地址再跟下一跳 */
        if (vpn) next = restoreUrl(next, vpn)
        fetchRaw(next, options, redirects + 1).then(resolve, reject)
        return
      }
      const chunks = []
      let size = 0
      res.on('data', (chunk) => {
        size += chunk.length
        if (size > 6 * 1024 * 1024) {
          req.destroy(new Error('响应过大'))
          return
        }
        chunks.push(chunk)
      })
      res.on('end', () => {
        resolve({
          status,
          url: vpn ? restoreUrl(target.href, vpn) : target.href,
          headers: res.headers,
          body: decompress(Buffer.concat(chunks), res.headers['content-encoding']),
        })
      })
      res.on('error', reject)
    })
    req.on('timeout', () => req.destroy(new Error('请求超时')))
    req.on('error', reject)
    req.end(payload || undefined)
  })
}

function sniffCharset(headers, buffer) {
  const ct = String(headers?.['content-type'] || '')
  let m = /charset=["']?([\w-]+)/i.exec(ct)
  if (m) return m[1].toLowerCase()
  const head = buffer.subarray(0, 3000).toString('latin1')
  m = /<meta[^>]+charset=["']?([\w-]+)/i.exec(head)
  if (m) return m[1].toLowerCase()
  return 'utf-8'
}

function decodeBody(buffer, headers) {
  const charset = sniffCharset(headers, buffer)
  try {
    return new TextDecoder(charset === 'gb2312' ? 'gbk' : charset).decode(buffer)
  } catch {
    return buffer.toString('utf8')
  }
}

/* ------------------------------------------------------------------ */
/* 三种列表解析                                                         */
/* ------------------------------------------------------------------ */

function parseHtmlList(html, baseUrl, source) {
  const out = []
  const seen = new Set()
  const pattern = /<a\b([^>]*)>([\s\S]*?)<\/a>/gi
  let match
  let linkFilter = null
  if (source.linkPattern) {
    try {
      linkFilter = new RegExp(source.linkPattern, 'i')
    } catch {
      linkFilter = null
    }
  }
  while ((match = pattern.exec(html)) !== null) {
    const attrs = match[1] || ''
    const hrefMatch = /href\s*=\s*("([^"]*)"|'([^']*)'|([^\s>]+))/i.exec(attrs)
    const href = hrefMatch ? hrefMatch[2] ?? hrefMatch[3] ?? hrefMatch[4] : ''
    if (!href) continue
    const rawTitle = match[2] || ''
    const text = cleanText(rawTitle)
    /* 与资讯类站点一样：先按原始文字判日期，再去掉栏目前缀 */
    if (!text || text.length < 4 || text.length > 300) continue
    const title = cleanTitle(text)
    if (!title || title.length < 4) continue
    if (NOISE_TITLES.has(title)) continue
    if (/^[\d\s.]+$/.test(title)) continue
    if (/^(javascript|mailto|tel):/i.test(href) || href.startsWith('#')) continue
    let absolute
    try {
      absolute = new URL(href, baseUrl).href
    } catch {
      continue
    }
    if (!/^https?:/i.test(absolute)) continue
    if (linkFilter && !linkFilter.test(absolute) && !linkFilter.test(title)) continue
    const start = Math.max(0, match.index - 180)
    const context = cleanText(html.slice(start, match.index + match[0].length + 180))
    const looksLikeNotice = NOTICE_WORDS.test(title)
    const date = findDate(text) || (looksLikeNotice ? findDate(context) : '')
    const detailHref = /\/info\//i.test(absolute) || /\/\d{3,}\.(?:s?html?)$/i.test(absolute)
    const accept = Boolean(date) || (detailHref && title.length >= 12) || (looksLikeNotice && title.length >= 6)
    if (!accept) continue
    const key = `${title}|${absolute}`
    if (seen.has(key)) continue
    seen.add(key)
    out.push({ title, url: absolute, publishedAt: date, context })
  }
  return out
}

/** RSS/Atom 里的时间：可能是 RFC822（Thu, 05 Nov 2026 …）也可能是 ISO。 */
function parseFeedDate(raw) {
  const text = cleanText(raw)
  if (!text) return ''
  const numeric = findDate(text)
  if (numeric) return numeric
  const parsed = new Date(text)
  if (!Number.isFinite(parsed.getTime())) return ''
  return isoDate(parsed.getFullYear(), parsed.getMonth() + 1, parsed.getDate())
}

function parseRss(xml, baseUrl) {
  const out = []
  const blocks = xml.split(/<item[\s>]/i).slice(1)
  const atomBlocks = xml.split(/<entry[\s>]/i).slice(1)
  const list = blocks.length ? blocks : atomBlocks
  for (const block of list) {
    const pick = (tag) => {
      const m = new RegExp(`<${tag}[^>]*>([\\s\\S]*?)</${tag}>`, 'i').exec(block)
      if (!m) return ''
      return cleanText(m[1].replace(/<!\[CDATA\[([\s\S]*?)\]\]>/g, '$1'))
    }
    const title = cleanTitle(pick('title'))
    if (!title) continue
    let link = ''
    const linkCdata = /<link[^>]*>([\s\S]*?)<\/link>/i.exec(block)
    if (linkCdata) link = cleanText(linkCdata[1].replace(/<!\[CDATA\[([\s\S]*?)\]\]>/g, '$1'))
    if (!link) {
      const hrefAttr = /<link[^>]+href=["']([^"']+)["']/i.exec(block)
      if (hrefAttr) link = hrefAttr[1]
    }
    let absolute = ''
    try {
      absolute = link ? new URL(link, baseUrl).href : ''
    } catch {
      absolute = ''
    }
    const pub = pick('pubDate') || pick('published') || pick('updated') || pick('dc:date') || pick('date')
    out.push({
      title,
      url: absolute || baseUrl,
      publishedAt: parseFeedDate(pub),
      context: excerptOf(pick('content:encoded') || pick('description') || pick('summary') || ''),
    })
  }
  return out
}

function excerptOf(html) {
  return cleanText(html).slice(0, 400)
}

function pickPath(value, path) {
  if (!path) return value
  let current = value
  for (const key of String(path).split('.')) {
    if (current == null) return undefined
    if (Array.isArray(current) && /^\d+$/.test(key)) current = current[Number(key)]
    else current = current[key]
  }
  return current
}

/** 在响应里找第一个「对象数组」，用于没填 itemsPath 的接口源。 */
function guessList(data) {
  if (Array.isArray(data)) return data
  if (!data || typeof data !== 'object') return null
  const candidates = [
    'd.data', 'd.list', 'd.rows', 'd.items', 'd.records', 'd.result', 'd',
    'data', 'list', 'rows', 'items', 'records', 'result', 'content',
    'data.list', 'data.rows', 'data.items', 'data.records',
  ]
  for (const path of candidates) {
    const value = pickPath(data, path)
    if (Array.isArray(value) && value.some((row) => row && typeof row === 'object')) return value
  }
  /* 再不行就广度优先找第一个对象数组 */
  const queue = [data]
  let guard = 0
  while (queue.length && guard < 200) {
    guard += 1
    const current = queue.shift()
    if (!current || typeof current !== 'object') continue
    for (const value of Object.values(current)) {
      if (Array.isArray(value) && value.some((row) => row && typeof row === 'object')) return value
      if (value && typeof value === 'object') queue.push(value)
    }
  }
  return null
}

function parseJsonList(text, source) {
  let data
  try {
    data = JSON.parse(text)
  } catch {
    return []
  }
  /* 填了 itemsPath 就按它来，取不到数组就自动猜一个 */
  let list = source.itemsPath ? pickPath(data, source.itemsPath) : null
  if (!Array.isArray(list)) list = guessList(data)
  if (!Array.isArray(list)) return []
  const map = source.map || {}
  const out = []
  for (const row of list.slice(0, 200)) {
    const title = cleanTitle(pickPath(row, map.title || 'title') ?? '')
    if (!title) continue
    let url = pickPath(row, map.url || map.link || 'url') ?? ''
    url = String(url || '')
    const itemId = String(pickPath(row, map.id || 'id') ?? '')
    /* 接口不给链接时，用模板拼一条人能点开的页面（{id} / {index} / {title}） */
    if (!url && source.urlTemplate) {
      url = String(source.urlTemplate)
        .replace(/\{id\}/g, encodeURIComponent(itemId))
        .replace(/\{index\}/g, String(out.length + 1))
        .replace(/\{title\}/g, encodeURIComponent(title))
    }
    if (url && !/^https?:/i.test(url)) {
      try {
        url = new URL(url, source.url).href
      } catch {
        url = ''
      }
    }
    out.push({
      itemId,
      title,
      url: url || '',
      publishedAt: findDate(String(pickPath(row, map.date || 'date') ?? '')),
      context: excerptOf(String(pickPath(row, map.summary || 'summary') ?? '')),
    })
  }
  return out
}

/* ------------------------------------------------------------------ */
/* 插件本体                                                             */
/* ------------------------------------------------------------------ */
/* ------------------------- 日程相关的小工具 ------------------------- */

/** 把各种写法的时间归一成 "YYYY-MM-DDTHH:mm"（本地时区语义）。 */
export function normalizeDue(value) {
  const text = String(value || '').trim()
  if (!text) return ''
  if (/^\d{4}-\d{2}-\d{2}$/.test(text)) return `${text}T23:59`
  if (/^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}/.test(text)) return text.replace(' ', 'T').slice(0, 16)
  const parsed = new Date(text)
  if (Number.isFinite(parsed.getTime())) {
    return `${parsed.getFullYear()}-${pad2(parsed.getMonth() + 1)}-${pad2(parsed.getDate())}T${pad2(parsed.getHours())}:${pad2(parsed.getMinutes())}`
  }
  return ''
}

export function describeMinutes(minutes) {
  const value = Number(minutes) || 0
  if (value === 0) return '准时'
  if (value % 1440 === 0) return `${value / 1440} 天前`
  if (value % 60 === 0) return `${value / 60} 小时前`
  return `${value} 分钟前`
}

/** 「还有 2 天 3 小时」这类人话倒计时。 */
export function countdownText(dueAt) {
  const date = new Date(normalizeDue(dueAt) || 'x')
  if (!Number.isFinite(date.getTime())) return ''
  const diff = date.getTime() - Date.now()
  const abs = Math.abs(diff)
  const days = Math.floor(abs / 86400000)
  const hours = Math.floor((abs % 86400000) / 3600000)
  const minutes = Math.floor((abs % 3600000) / 60000)
  const body = days > 0 ? `${days} 天 ${hours} 小时` : hours > 0 ? `${hours} 小时 ${minutes} 分` : `${minutes} 分钟`
  return diff >= 0 ? `还有 ${body}` : `已过 ${body}`
}

export {
  DEFAULT_KEYWORDS,
  DEFAULT_SOURCES,
  NOTICE_WORDS,
  emptyStore,
  numberOr,
  sha1,
  stripTags,
  decodeEntities,
  cleanText,
  cleanTitle,
  stripRedHeader,
  pad2,
  isoDate,
  findDate,
  findDeadline,
  cut,
  findContact,
  extractMainContent,
  pickTags,
  daysUntil,
  scoreNotice,
  ruleSummary,
  resolveAddress,
  decompress,
  fetchRaw,
  sniffCharset,
  decodeBody,
  parseHtmlList,
  parseRss,
  excerptOf,
  pickPath,
  parseJsonList,
  WEBVPN_GATEWAY,
  DEFAULT_WEBVPN,
  DEFAULT_LOGIN_PAGE_PATTERN,
  isWebvpnEnabled,
  rewriteUrl,
  restoreUrl,
  restoreUrlsInText,
  webvpnSessionExpired,
}
