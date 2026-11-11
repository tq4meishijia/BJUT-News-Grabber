/**
 * 统一身份认证（CAS）自动登录 + 用 Windows DPAPI 加密保存密码。
 *
 * 流程：GET 登录页（拿 execution 和会话 Cookie）→ POST 表单 → 跟随 302 回到门户
 * → 一路累积 Cookie（门户会在这个过程里下发自己的会话 Cookie）。
 *
 * 认证入口不写死：`casBase` 是登录页所在站点，`service` 是登录成功后要回到的地址
 * （门户/办事大厅首页之类）。换学校、换系统只需要改这两个值。
 *
 * 验证码默认隐藏；真遇到验证码时返回明确原因，让用户手动更新登录态。
 */
import { spawnSync } from 'node:child_process'
import { fetchRaw, decodeBody, cleanText, restoreUrl } from './core.mjs'

function hostOf(value) {
  try {
    return new URL(String(value || '')).host
  } catch {
    return ''
  }
}

/* ------------------------------ Cookie 罐 ------------------------------ */

function parseSetCookie(headers) {
  return [].concat(headers['set-cookie'] || []).map((raw) => {
    const parts = String(raw).split(';')
    const pair = parts[0]
    const index = pair.indexOf('=')
    if (index <= 0) return null
    const domainAttr = parts
      .slice(1)
      .map((part) => part.trim())
      .find((part) => /^domain=/i.test(part))
    const domain = domainAttr ? domainAttr.slice(domainAttr.indexOf('=') + 1).replace(/^\./, '').toLowerCase() : ''
    return { name: pair.slice(0, index), value: pair.slice(index + 1), domain }
  }).filter(Boolean)
}

/** 登录走完但门户没下发会话 Cookie 时给用户的提示。 */
export const NO_PORTAL_SESSION =
  '登录走通了，但门户没有下发自己的会话 Cookie（只拿到认证服务器的）：把「登录后返回的地址」改成门户的认证回调地址，' +
  '通常形如 https://门户/common/actionCasLogin?redirect_url=https://门户/首页'

function jarToHeader(jar) {
  return [...jar.entries()].map(([name, value]) => `${name}=${value}`).join('; ')
}

/** 自带 Cookie 罐、自己跟随重定向的请求。 */
async function jarRequest(url, { jar, method = 'GET', body, headers = {}, maxRedirects = 8, onSetCookie, webvpn = null }) {
  let current = url
  let currentMethod = method
  let currentBody = body
  /* 网关自己那层会话 Cookie 由配置单独提供，不收进门户会话罐；
     用 hostname（不含端口）比对，Cookie 的 Domain 属性本来就不带端口 */
  let gatewayHost = ''
  if (webvpn && webvpn.enabled && webvpn.gateway) {
    try {
      gatewayHost = new URL(webvpn.gateway).hostname.toLowerCase()
    } catch {
      gatewayHost = ''
    }
  }
  for (let hop = 0; hop <= maxRedirects; hop += 1) {
    const cookie = jarToHeader(jar)
    const response = await fetchRaw(current, {
      method: currentMethod,
      body: currentBody,
      noFollow: true,
      timeoutMs: 20_000,
      webvpn,
      headers: {
        cookie,
        'user-agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36',
        accept: 'text/html,application/xhtml+xml,*/*;q=0.8',
        'accept-language': 'zh-CN,zh;q=0.9',
        ...headers,
      },
    })
    const pairs = parseSetCookie(response.headers).filter((pair) => {
      /* 关闭 WebVPN 时保持原行为：所有 Cookie 照旧进罐 */
      if (!gatewayHost) return true
      const domain = String(pair.domain || '').toLowerCase()
      /* 无 Domain 的是 host-only Cookie，WebVPN 下实际连接主机是网关，归属网关而非门户；
         带网关 Domain 的同样过滤。只有带门户 Domain 的才证明门户会话真的建立了。 */
      if (!domain) return false
      return !(domain === gatewayHost || domain.endsWith(`.${gatewayHost}`))
    })
    for (const pair of pairs) jar.set(pair.name, pair.value)
    /* current 始终是还原后的门户/CAS 原始地址，与过滤后剩下的门户 Cookie 归属一致 */
    if (onSetCookie && pairs.length) onSetCookie(hostOf(current), pairs)
    const location = response.headers.location
    if (response.status >= 300 && response.status < 400 && location) {
      let next = new URL(location, current).href
      /* 网关回的 Location 是改写过的，还原成门户/CAS 原始地址，下一跳再统一重写出站 */
      if (webvpn) next = restoreUrl(next, webvpn)
      current = next
      currentMethod = 'GET'
      currentBody = undefined
      continue
    }
    return { ...response, url: current }
  }
  throw new Error('重定向次数过多')
}

/* ------------------------------ CAS 登录 ------------------------------ */

/**
 * 从登录页里取 execution。
 * 不依赖属性顺序、引号有无，也不依赖它是 hidden 还是普通 input。
 */
function extractExecution(html) {
  const tags = String(html || '').match(/<input\b[^>]*>/gi) || []
  for (const tag of tags) {
    const name = /\bname\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/i.exec(tag)
    const id = /\bid\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/i.exec(tag)
    const key = (name?.[1] || name?.[2] || name?.[3] || id?.[1] || id?.[2] || id?.[3] || '').toLowerCase()
    if (key !== 'execution') continue
    const value = /\bvalue\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/i.exec(tag)
    const raw = value?.[1] || value?.[2] || value?.[3] || ''
    if (raw) return raw
  }
  return ''
}

/** 页面看起来是「你已经登录过了」的样子。 */
function looksLoggedIn(html) {
  const text = cleanText(String(html || '')).slice(0, 4000)
  return /已登录|已登陆|欢迎回来|继续访问|正在跳转|跳转中/.test(text)
}

function pageTitle(html) {
  const match = /<title[^>]*>([\s\S]*?)<\/title>/i.exec(String(html || ''))
  return match ? cleanText(match[1]).slice(0, 40) : ''
}

function needsCaptcha(html) {
  if (/<input[^>]+name=["']?captcha["']/i.test(html)) {
    /* 输入框存在不算，得看它显示没显示 */
    const style = /id=["']?captchaParent["']?[^>]*style=["']([^"']*)["']/i.exec(html)
    if (!style || !/display\s*:\s*none/i.test(style[1])) return true
  }
  return /请输入验证码|验证码错误/.test(html)
}

function extractError(html) {
  const div = /<div[^>]*id=["']?errorDiv["']?[^>]*>([\s\S]{0,300}?)<\/div>/i.exec(html)
  const text = div ? cleanText(div[1]) : ''
  if (text) return text
  const p = /<p[^>]*class=["'][^"']*error[^"']*["'][^>]*>([\s\S]{0,200}?)<\/p>/i.exec(html)
  return p ? cleanText(p[1]) : ''
}

/**
 * 登录统一身份认证，返回可以直接给门户接口用的 Cookie 头和明细。
 * @param {{casBase: string, service: string, username: string, password: string, signal?: AbortSignal}} input
 * @returns {Promise<{ok: true, cookie: string, cookies: Record<string,string>} | {ok: false, reason: string}>}
 */
export async function casLogin({ casBase, service, username, password, signal, webvpn = null }) {
  if (!username || !password) return { ok: false, reason: '账号或密码为空' }
  if (!casBase || !service) {
    return { ok: false, reason: '还没配置「认证入口地址」和「登录后返回的地址」（设置页可以填）' }
  }
  const vpn = webvpn && webvpn.enabled && webvpn.gateway ? webvpn : null
  /* 走 WebVPN 时把所有地址统一还原成门户/CAS 原始地址：hostOf 与 portalCookies
     仍按门户域名判定，不会因为请求 host 变成网关而误报 NO_PORTAL_SESSION */
  const effectiveCasBase = vpn ? restoreUrl(casBase, vpn) : casBase
  const effectiveService = vpn ? restoreUrl(service, vpn) : service
  const casHost = hostOf(effectiveCasBase)
  const portalHost = hostOf(effectiveService)
  if (!casHost || !portalHost) return { ok: false, reason: '认证入口或返回地址不是合法网址' }
  const jar = new Map()
  const loginUrl = `${effectiveCasBase.replace(/\/+$/, '')}/login?service=${encodeURIComponent(effectiveService)}`
  /* 记录门户域名下发过哪些 Cookie：没有就说明会话没真正建立 */
  const portalCookies = new Set()
  const onSetCookie = (host, pairs) => {
    for (const pair of pairs) {
      const domain = pair.domain || ''
      const fromPortal =
        host === portalHost ||
        domain === portalHost ||
        (domain && portalHost.endsWith(`.${domain}`)) ||
        (host && host.endsWith(`.${portalHost}`))
      if (fromPortal) portalCookies.add(pair.name)
    }
  }
  try {
    const page = await jarRequest(loginUrl, { jar, onSetCookie, webvpn: vpn })
    let html = decodeBody(page.body, page.headers)
    let execution = extractExecution(html)
    const hasPortalSession = () => portalCookies.size > 0
    const successWithJar = () => {
      const cookie = jarToHeader(jar)
      if (!cookie) return null
      if (!hasPortalSession()) return { ok: false, reason: NO_PORTAL_SESSION }
      return { ok: true, cookie, cookies: Object.fromEntries(jar) }
    }
    if (!execution && hostOf(page.url) !== casHost) {
      const done = successWithJar()
      if (done) return done
    }

    /* 有些 CAS 先给一个「正在跳转/已登录」的中间页，再取一次就能拿到真正的表单 */
    if (!execution) {
      const again = await jarRequest(loginUrl, { jar, onSetCookie, webvpn: vpn })
      const againHtml = decodeBody(again.body, again.headers)
      const againExecution = extractExecution(againHtml)
      if (!againExecution && (looksLoggedIn(html) || looksLoggedIn(againHtml) || hostOf(again.url) !== casHost)) {
        const done = successWithJar()
        if (done) return done
      }
      if (againExecution) {
        html = againHtml
        execution = againExecution
      }
    }

    if (!execution) {
      if (needsCaptcha(html)) {
        return { ok: false, reason: '统一身份认证要求输入验证码，自动登录不可用，请手动更新登录态' }
      }
      const title = pageTitle(html)
      return {
        ok: false,
        reason:
          `登录页里没找到 execution 字段${title ? `（页面标题：${title}）` : ''}：` +
          '确认「认证入口地址」填的是登录页本身（一般形如 https://cas.xxx.edu.cn），或过一会儿再试',
      }
    }
    if (needsCaptcha(html)) {
      return { ok: false, reason: '统一身份认证要求输入验证码，自动登录不可用，请手动更新登录态' }
    }

    const form = new URLSearchParams({
      username,
      password,
      type: 'username_password',
      execution,
      _eventId: 'submit',
      submit: '登录',
    }).toString()

    const posted = await jarRequest(loginUrl, {
      jar,
      onSetCookie,
      webvpn: vpn,
      method: 'POST',
      body: form,
      headers: {
        'content-type': 'application/x-www-form-urlencoded',
        referer: loginUrl,
        origin: new URL(effectiveCasBase).origin,
      },
    })
    const postedHtml = decodeBody(posted.body, posted.headers)

    if (hostOf(posted.url) === casHost && /\/login/.test(posted.url)) {
      const error = extractError(postedHtml)
      if (needsCaptcha(postedHtml)) {
        return { ok: false, reason: '登录失败：账号密码错误次数偏多，需要验证码，请手动更新登录态' }
      }
      return { ok: false, reason: error ? `登录失败：${error}` : '登录失败：仍停留在登录页（账号或密码不对）' }
    }

    /* 到这一步应该已经带着 ticket 回到门户，Cookie 罐里有门户会话 */
    const cookie = jarToHeader(jar)
    if (!portalCookies.size) return { ok: false, reason: NO_PORTAL_SESSION }
    if (hostOf(posted.url) !== portalHost) {
      return { ok: false, reason: `登录流程走完了，但没有回到「${portalHost}」，检查「登录后返回的地址」` }
    }
    if (!cookie) return { ok: false, reason: '登录成功但没拿到任何 Cookie' }
    return { ok: true, cookie, cookies: Object.fromEntries(jar) }
  } catch (error) {
    return { ok: false, reason: `登录请求失败：${error?.message || error}` }
  }
}

/** 由门户首页推出常见的「认证回调」候选地址，用于自动纠正返回地址。 */
export function casCallbackCandidates(service) {
  try {
    const url = new URL(service)
    return [
      `${url.origin}/common/actionCasLogin?redirect_url=${encodeURIComponent(service)}`,
      `${url.origin}/casLogin?redirect_url=${encodeURIComponent(service)}`,
    ]
  } catch {
    return []
  }
}

/* ------------------------------ 密码加解密 ------------------------------ */

const PS_ENCRYPT =
  "$s = ConvertTo-SecureString -String $env:CN_PWD -AsPlainText -Force; ConvertFrom-SecureString -SecureString $s"

const PS_DECRYPT =
  "$s = ConvertTo-SecureString -String $env:CN_ENC; [Runtime.InteropServices.Marshal]::PtrToStringAuto([Runtime.InteropServices.Marshal]::SecureStringToBSTR($s))"

/** 用当前 Windows 用户身份加密（DPAPI），换台机器或换用户都解不开。 */
export function encryptPassword(plain) {
  if (!plain) return ''
  if (process.platform !== 'win32') return ''
  const result = spawnSync(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-Command', PS_ENCRYPT],
    { encoding: 'utf8', windowsHide: true, env: { ...process.env, CN_PWD: plain } },
  )
  const text = (result.stdout || '').trim()
  if (result.status !== 0 || !text) return ''
  return text
}

export function decryptPassword(encrypted) {
  if (!encrypted) return ''
  if (process.platform !== 'win32') return ''
  const result = spawnSync(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-Command', PS_DECRYPT],
    { encoding: 'utf8', windowsHide: true, env: { ...process.env, CN_ENC: encrypted } },
  )
  if (result.status !== 0) return ''
  return (result.stdout || '').replace(/\r?\n$/, '')
}
