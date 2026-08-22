const { contextBridge, ipcRenderer } = require('electron')

let nextListenerId = 1
const listeners = new Map()

contextBridge.exposeInMainWorld('cfdEditorElectron', {
  invoke: async (command, args = {}) => {
    const response = await ipcRenderer.invoke('cfd-editor:invoke', { command, args })
    if (response.ok) return response.result
    const error = new Error(response.error?.message || 'Electron editor command failed')
    Object.assign(error, response.error || {})
    throw error
  },
  subscribe: (event, handler) => {
    const id = nextListenerId++
    const listener = (_message, receivedEvent, payload) => {
      if (receivedEvent === event) handler(payload)
    }
    listeners.set(id, listener)
    ipcRenderer.on('cfd-editor:event', listener)
    return id
  },
  unsubscribe: id => {
    const listener = listeners.get(id)
    if (listener) ipcRenderer.removeListener('cfd-editor:event', listener)
    listeners.delete(id)
  },
  openDialog: options => ipcRenderer.invoke('cfd-editor:open-dialog', options),
})
