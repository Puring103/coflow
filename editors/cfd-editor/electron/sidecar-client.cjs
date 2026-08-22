const { spawn } = require('node:child_process')
const readline = require('node:readline')

class SidecarClient {
  constructor(executable, options = {}) {
    this.nextId = 1
    this.pending = new Map()
    this.onEvent = options.onEvent || (() => {})
    this.child = spawn(executable, [], {
      cwd: options.cwd,
      windowsHide: true,
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    this.child.stderr.setEncoding('utf8')
    this.child.stderr.on('data', text => process.stderr.write(`[cfd-editor-sidecar] ${text}`))
    readline.createInterface({ input: this.child.stdout }).on('line', line => this.handleLine(line))
    this.child.on('exit', (code, signal) => {
      const error = new Error(`CFD editor sidecar exited (${signal || code})`)
      for (const { reject } of this.pending.values()) reject(error)
      this.pending.clear()
    })
  }

  request(command, args = {}) {
    const id = this.nextId++
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject })
      this.child.stdin.write(`${JSON.stringify({ id, command, args })}\n`, error => {
        if (!error) return
        this.pending.delete(id)
        reject(error)
      })
    })
  }

  handleLine(line) {
    let message
    try {
      message = JSON.parse(line)
    } catch (error) {
      process.stderr.write(`[cfd-editor-sidecar] invalid JSON: ${error.message}\n`)
      return
    }
    if (message.event) {
      this.onEvent(message.event, message.payload)
      return
    }
    const pending = this.pending.get(message.id)
    if (!pending) return
    this.pending.delete(message.id)
    if (message.error) {
      const error = new Error(message.error.message || 'CFD editor sidecar command failed')
      Object.assign(error, message.error)
      pending.reject(error)
    } else {
      pending.resolve(message.result)
    }
  }

  close() {
    if (!this.child.killed) this.child.kill()
  }
}

module.exports = { SidecarClient }
