const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { SidecarClient } = require('./sidecar-client.cjs')

const executable = process.env.CFD_EDITOR_SIDECAR || path.resolve(
  __dirname,
  '../../../target/debug',
  process.platform === 'win32' ? 'cfd-editor-sidecar.exe' : 'cfd-editor-sidecar',
)

async function main() {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), 'cfd-electron-sidecar-'))
  fs.mkdirSync(path.join(project, 'data'))
  fs.writeFileSync(path.join(project, 'schema.cft'), 'type Item { name: string; }\n')
  fs.writeFileSync(path.join(project, 'data/items.cfd'), 'sword: Item { name: "Sword" }\n')
  fs.writeFileSync(
    path.join(project, 'coflow.yaml'),
    'schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n',
  )
  const events = []
  const client = new SidecarClient(executable, {
    cwd: path.resolve(__dirname, '../../..'),
    onEvent: (event, payload) => events.push({ event, payload }),
  })
  try {
    assert.equal(await client.request('ping'), 'pong')
    await assert.rejects(client.request('unknown_command'), /unknown editor command/)
    const snapshot = await client.request('load_project', {
      yamlPath: path.join(project, 'coflow.yaml'),
    })
    const records = await client.request('get_file_records', {
      sessionId: snapshot.session_id,
      filePath: 'data/items.cfd',
    })
    assert.equal(records.records.length, 1)
    assert.equal(records.records[0].coordinate.key, 'sword')
    fs.writeFileSync(path.join(project, 'data/items.cfd'), 'sword: Item { name: "Blade" }\n')
    await waitFor(() => events.some(item => item.event === 'project_changed'), 4_000)
    await client.request('close_session', { sessionId: snapshot.session_id })
    process.stdout.write('electron-sidecar-client=ok\n')
  } finally {
    client.close()
    fs.rmSync(project, { recursive: true, force: true })
  }
}

async function waitFor(predicate, timeoutMs) {
  const started = Date.now()
  while (!predicate()) {
    if (Date.now() - started >= timeoutMs) throw new Error('timed out waiting for sidecar event')
    await new Promise(resolve => setTimeout(resolve, 25))
  }
}

main().catch(error => {
  console.error(error)
  process.exitCode = 1
})
