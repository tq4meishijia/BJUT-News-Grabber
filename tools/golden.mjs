#!/usr/bin/env node
/**
 * P0 · 基线冻结脚本。
 *
 * 作用：用现有 Node 实现跑一批「录制下来的真实响应」，把解析 / 抽取 / 打分 / 规则摘要的
 * 结果导出成 JSON，供 P1 的 Rust 实现做 golden 对照。整个过程不联网、不读 store.json。
 *
 *   node tools/golden.mjs --fixtures fixtures/raw --out fixtures/expected
 *   node tools/golden.mjs --fixtures fixtures/raw --out fixtures/expected --pretty
 *
 * 录制约定（文件名即用例名）：
 *   <kind>__<name>.<ext>            kind = html | rss | json
 *   html__portal-list-1.html        → parseHtmlList
 *   rss__sspai.xml                  → parseRss
 *   json__bjut-news.json            → parseJsonList
 *   html__portal-list-1.meta.json   → 可选，覆盖 baseUrl / source 配置
 *
 * meta 字段（全部可选）：
 *   { "baseUrl": "https://my.bjut.edu.cn/site/index",
 *     "source": { "url": "...", "itemsPath": "d.data", "map": {...},
 *                 "urlTemplate": "...", "linkPattern": "" },
 *     "keywords": ["四级", "选课"],
 *     "content": "详情页正文，用于对照抽取与打分（不给则按 store.mjs 的 bodyText 规则取）" }
 */
import { readdir, readFile, mkdir, writeFile } from 'node:fs/promises'
import { basename, extname, join, resolve } from 'node:path'
import {
  DEFAULT_KEYWORDS,
  cleanTitle,
  findDate,
  findDeadline,
  findContact,
  pickTags,
  parseHtmlList,
  parseJsonList,
  parseRss,
  ruleSummary,
  scoreNotice,
} from '../core.mjs'

function parseArgs(argv) {
  const args = { fixtures: 'fixtures/raw', out: 'fixtures/expected', pretty: false }
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]
    if (arg === '--fixtures') args.fixtures = argv[++i]
    else if (arg === '--out') args.out = argv[++i]
    else if (arg === '--pretty') args.pretty = true
    else if (arg === '--help' || arg === '-h') args.help = true
  }
  return args
}

const KINDS = new Set(['html', 'rss', 'json'])

function splitName(file) {
  const ext = extname(file)
  const stem = basename(file, ext)
  const index = stem.indexOf('__')
  if (index < 0) return null
  const kind = stem.slice(0, index)
  if (!KINDS.has(kind)) return null
  return { kind, name: stem.slice(index + 2), ext: ext.toLowerCase() }
}

async function readMeta(dir, name) {
  try {
    return JSON.parse(await readFile(join(dir, `${name}.meta.json`), 'utf8'))
  } catch {
    return {}
  }
}

/**
 * 把一条原始条目补全成与 store.mjs 第 542-561 行一致的形状。
 *
 * 注意 bodyText 的取法必须与 store.mjs 一致：
 *   content（详情页正文）优先；没有正文时，只有 json 源可以用列表摘要兜底，
 *   html/rss 的 context 是页面碎片，不能当正文（否则摘要会变成导航文字）。
 * 详情页正文由录制方通过 meta.content 提供，这样「列表阶段」与「补全阶段」可分开对照。
 */
function enrich(item, keywords, bodyText) {
  const text = `${item.title} ${bodyText || ''}`
  const tags = pickTags(text, keywords)
  const deadline = findDeadline(text)
  const contact = findContact(bodyText || '')
  const rule = ruleSummary({ title: item.title, content: bodyText || '', tags, deadline, contact })
  return {
    raw: {
      itemId: item.itemId || '',
      title: item.title,
      titleCleaned: cleanTitle(item.title),
      url: item.url || '',
      publishedAt: item.publishedAt || '',
      dateGuessed: findDate(item.title),
      context: item.context || '',
    },
    extract: { tags, deadline, contact },
    score: scoreNotice({ title: item.title, content: text, tags, deadline, keywords }),
    rule,
  }
}

async function run() {
  const args = parseArgs(process.argv.slice(2))
  if (args.help) {
    console.log('node tools/golden.mjs --fixtures <dir> --out <dir> [--pretty]')
    process.exit(0)
  }

  const fixturesDir = resolve(args.fixtures)
  const outDir = resolve(args.out)
  let files = []
  try {
    files = (await readdir(fixturesDir)).filter((file) => splitName(file) && !file.endsWith('.meta.json'))
  } catch {
    console.error(`读不到用例目录：${fixturesDir}`)
    console.error('先按 docs/rust-tauri-refactor-plan.md §9 P0 录制响应，文件名形如 html__portal-list-1.html')
    process.exit(1)
  }
  if (!files.length) {
    console.error(`用例目录为空：${fixturesDir}`)
    process.exit(1)
  }

  await mkdir(outDir, { recursive: true })
  const summary = []

  for (const file of files.sort()) {
    const info = splitName(file)
    const body = await readFile(join(fixturesDir, file), 'utf8')
    const meta = await readMeta(fixturesDir, basename(file, extname(file)))
    const baseUrl = meta.baseUrl || 'https://example.edu.cn/'
    const source = { url: baseUrl, type: info.kind, ...(meta.source || {}) }
    const keywords = Array.isArray(meta.keywords) && meta.keywords.length ? meta.keywords : DEFAULT_KEYWORDS

    let items = []
    try {
      if (info.kind === 'html') items = parseHtmlList(body, baseUrl, source)
      else if (info.kind === 'rss') items = parseRss(body, baseUrl)
      else items = parseJsonList(body, source)
    } catch (error) {
      console.error(`[失败] ${file}：${error?.message || error}`)
      summary.push({ file, ok: false, count: 0 })
      continue
    }

    const useContextAsBody = info.kind === 'json'
    const payload = {
      fixture: file,
      kind: info.kind,
      baseUrl,
      generatedAt: new Date().toISOString(),
      bodySource: typeof meta.content === 'string' ? 'meta' : useContextAsBody ? 'context' : 'none',
      count: items.length,
      items: items.map((item) => {
        const bodyText =
          typeof meta.content === 'string' ? meta.content : useContextAsBody ? item.context || '' : ''
        return enrich(item, keywords, bodyText)
      }),
    }
    const target = join(outDir, `${basename(file, extname(file))}.json`)
    await writeFile(target, `${JSON.stringify(payload, null, args.pretty ? 2 : 0)}\n`, 'utf8')
    summary.push({ file, ok: true, count: items.length })
  }

  const ok = summary.filter((row) => row.ok)
  const empty = ok.filter((row) => row.count === 0)
  console.log(`导出 ${ok.length}/${summary.length} 个用例 → ${outDir}`)
  for (const row of summary) console.log(`  ${row.ok ? '✓' : '✗'} ${row.file}  ${row.count} 条`)
  if (empty.length) {
    console.warn(`\n注意：${empty.length} 个用例解析出 0 条，通常是 baseUrl 或 source.map 配置不对，先修用例再冻结。`)
  }
  process.exit(ok.length === summary.length ? 0 : 1)
}

run().catch((error) => {
  console.error(error)
  process.exit(1)
})
