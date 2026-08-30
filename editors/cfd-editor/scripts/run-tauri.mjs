import { createHash } from 'node:crypto'
import { mkdirSync, realpathSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const editorRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repositoryRoot = realpathSync(resolve(editorRoot, '../..'))
const targetDir = process.env.CARGO_TARGET_DIR || defaultTargetDir(repositoryRoot)
const tauriCli = resolve(editorRoot, 'node_modules/@tauri-apps/cli/tauri.js')

mkdirSync(targetDir, { recursive: true })
process.stderr.write(`[cfd-editor] Cargo target: ${targetDir}\n`)

const child = spawn(process.execPath, [tauriCli, ...process.argv.slice(2)], {
  cwd: editorRoot,
  env: { ...process.env, CARGO_TARGET_DIR: targetDir },
  stdio: 'inherit',
})

child.on('error', error => {
  process.stderr.write(`[cfd-editor] Failed to start Tauri CLI: ${error.message}\n`)
  process.exitCode = 1
})

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
    return
  }
  process.exitCode = code ?? 1
})

function defaultTargetDir(root) {
  const cacheRoot = process.env.XDG_CACHE_HOME
    || process.env.LOCALAPPDATA
    || (process.platform === 'darwin'
      ? join(homedir(), 'Library/Caches')
      : join(homedir(), '.cache'))
  const repositoryId = createHash('sha256').update(root).digest('hex').slice(0, 12)
  return join(cacheRoot, 'coflow', 'targets', repositoryId)
}
