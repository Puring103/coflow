import { createServer } from 'vite'
import { chromium } from 'playwright'
import { readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
const root = fileURLToPath(new URL('../..', import.meta.url))
const plain = readFileSync(process.env.WIRE_PLAIN ?? join(tmpdir(), 'coflow-wire.plain.json'), 'utf8')
const transfer = readFileSync(process.env.WIRE_TRANSFER ?? join(tmpdir(), 'coflow-wire.transfer.json'), 'utf8')
const server = await createServer({ root, logLevel: 'error', server: { port: 0, host: '127.0.0.1' } })
await server.listen()
const browser = await chromium.launch({ headless: true })
try {
  const page = await browser.newPage()
  await page.goto(server.resolvedUrls.local[0])
  // 测量不包含工具传入字符串的开销；只计解析、整数归一化和描述恢复。
  const result = await page.evaluate(async ({plain, transfer}) => {
    const { fromIpc } = await import('/src/wire/ipc.ts')
    const { decodeFileRecords } = await import('/src/wire/fileRecordsTransfer.ts')
    const measure = (text, compact) => {
      const start = performance.now()
      const parsed = JSON.parse(text)
      const parse = performance.now()
      const normalized = fromIpc(parsed)
      const normalize = performance.now()
      const decoded = compact ? decodeFileRecords(normalized) : normalized
      const end = performance.now()
      return { parsed: decoded, ms: { parse: parse - start, normalize: normalize - parse, decode: end - normalize, total: end - start } }
    }
    const samples = []
    for (let index = 0; index < 5; index++) {
      for (const compact of index % 2 ? [true, false] : [false, true]) {
        const { ms } = measure(compact ? transfer : plain, compact)
        samples.push({compact, ms})
      }
    }
    const original = measure(plain, false).parsed
    const compact = measure(transfer, true).parsed
    const replacer = (_, value) => typeof value === 'bigint' ? value.toString() : value
    if (JSON.stringify(original, replacer) !== JSON.stringify(compact, replacer)) throw new Error('Snapshot round trip changed content')
    const unique = new Set(compact.records.flatMap(row => row.fields.map(field => field.annotation).filter(Boolean))).size
    return { samples, rows: compact.records.length, uniqueTopLevelAnnotations: unique, roundTrip: true }
  }, {plain, transfer})
  const output = { plainBytes: Buffer.byteLength(plain), transferBytes: Buffer.byteLength(transfer), browser: browser.version(), ...result }
  writeFileSync(process.env.WIRE_PERF_OUTPUT ?? join(tmpdir(), 'coflow-wire-performance.json'), JSON.stringify(output, null, 2))
  console.log(JSON.stringify(output))
} finally { await browser.close(); await server.close() }
