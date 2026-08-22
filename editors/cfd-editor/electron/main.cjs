const { app, BrowserWindow, dialog, ipcMain } = require('electron')
const path = require('node:path')
const { SidecarClient } = require('./sidecar-client.cjs')

let mainWindow
let sidecar

function sidecarExecutable() {
  if (process.env.CFD_EDITOR_SIDECAR) return process.env.CFD_EDITOR_SIDECAR
  const executable = process.platform === 'win32' ? 'cfd-editor-sidecar.exe' : 'cfd-editor-sidecar'
  return app.isPackaged
    ? path.join(process.resourcesPath, executable)
    : path.resolve(__dirname, '../../../target/debug', executable)
}

function createWindow() {
  mainWindow = new BrowserWindow({
    title: 'CFD Editor (Electron Preview)',
    width: 1280,
    height: 820,
    minWidth: 900,
    minHeight: 600,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  })
  mainWindow.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
  mainWindow.webContents.on('will-navigate', event => event.preventDefault())
  mainWindow.loadFile(path.join(__dirname, '../frontend/dist/index.html'))
  if (process.env.CFD_EDITOR_ELECTRON_SMOKE === '1') {
    mainWindow.webContents.once('did-finish-load', async () => {
      try {
        const result = await mainWindow.webContents.executeJavaScript(
          'window.cfdEditorElectron.invoke("ping")',
        )
        if (result !== 'pong') throw new Error(`unexpected sidecar response: ${result}`)
        const rendered = await mainWindow.webContents.executeJavaScript(
          'document.querySelector("#root")?.childElementCount > 0',
        )
        if (!rendered) throw new Error('React frontend did not render')
        process.stdout.write('electron-window-smoke=ok\n')
      } catch (error) {
        console.error(error)
        process.exitCode = 1
      } finally {
        app.quit()
      }
    })
  }
}

app.whenReady().then(() => {
  sidecar = new SidecarClient(sidecarExecutable(), {
    cwd: path.resolve(__dirname, '../../..'),
    onEvent: (event, payload) => mainWindow?.webContents.send('cfd-editor:event', event, payload),
  })
  ipcMain.handle('cfd-editor:invoke', async (_event, request) => {
    try {
      return { ok: true, result: await sidecar.request(request.command, request.args) }
    } catch (error) {
      return {
        ok: false,
        error: {
          kind: error.kind || 'other',
          message: error.message || String(error),
          diagnostics: error.diagnostics || [],
        },
      }
    }
  })
  ipcMain.handle('cfd-editor:open-dialog', async (_event, options = {}) => {
    const result = await dialog.showOpenDialog(mainWindow, {
      properties: options.directory ? ['openDirectory'] : ['openFile'],
      filters: options.filters,
    })
    return result.canceled ? null : result.filePaths[0] || null
  })
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

app.on('before-quit', () => sidecar?.close())
