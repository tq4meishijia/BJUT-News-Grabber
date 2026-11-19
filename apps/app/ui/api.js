/* Tauri 适配层：fetch('/api/action') → invoke('act')，保留原 op 名与载荷（plan：api.ts 等价实现，无构建链）。 */

const T = window.__TAURI__ || {}

/** 启动与重连：GET /api/state 等价。 */
export async function fetchState() {
  return await T.core.invoke('get_state')
}

/** 23 个 op 的统一入口：POST /api/action 等价（op 名与载荷与 Node 版一致）。 */
export async function api(op, payload = {}) {
  return await T.core.invoke('act', { req: { op, ...payload } })
}

/** 登录向导：三模式开始（WebView | Password | ImportCookie）。 */
export async function authBegin(scope, mode) {
  return await T.core.invoke('auth_begin', { scope, mode })
}

/** 登录向导：WebVPN 网关窗口登录（登录后自动写入网关 Cookie）。 */
export async function authWebvpnLogin() {
  return await T.core.invoke('auth_webvpn_login')
}

/** 登录向导：中途取消。 */
export async function authCancel(ticket) {
  return await T.core.invoke('auth_cancel', { ticket: ticket || '' })
}

/** 粘贴 Cookie 兜底。scope='gateway' 写网关，其余写同门户数据源。 */
export async function authImportCookie(scope, cookie) {
  return await T.core.invoke('auth_import_cookie', { scope, cookie })
}

/** 导出 store.json（不含登录态/密钥）。 */
export async function exportData(path) {
  return await T.core.invoke('export_data', { path })
}

/** 导入 store.json。 */
export async function importData(path) {
  return await T.core.invoke('import_data', { path })
}

/** 订阅后端事件（5 通道），返回取消函数。 */
export function listenTauri(event, handler) {
  if (!T.event || !T.event.listen) return () => {}
  let unlisten = null
  T.event.listen(event, (e) => handler(e.payload)).then((fn) => {
    unlisten = fn
  })
  return () => {
    if (unlisten) unlisten()
  }
}

/** 外链打开（opener 插件，S5 白名单）。 */
export function openExternal(url) {
  try {
    if (T.opener && T.opener.openUrl) {
      T.opener.openUrl(url)
      return
    }
  } catch {
    /* 回退 webview */
  }
  window.open(url, '_blank', 'noopener')
}

/** 打开本地附件（走 open_attachment 命令，Rust 侧校验路径白名单）。 */
export async function openAttachment(path) {
  return await T.core.invoke('open_attachment', { path })
}

/** 桌面通知（notification 插件；失败时 UI 内提示兜底）。 */
export async function notifySystem(title, body) {
  try {
    if (T.notification && T.notification.sendNotification) {
      T.notification.sendNotification({ title, body })
      return true
    }
  } catch {
    /* 权限未给 */
  }
  return false
}
