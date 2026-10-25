import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ desktop: true, read: vi.fn(), write: vi.fn() }))
vi.mock('./tauriEnv', () => ({ get isTauri() { return mocks.desktop } }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readText: mocks.read, writeText: mocks.write }))
import { readClipboardText, writeClipboardText } from './clipboard'

describe('clipboard host routing', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.desktop = true
    vi.unstubAllGlobals()
  })

  it('uses native clipboard without accessing browser permissions on desktop', async () => {
    vi.stubGlobal('navigator', { get clipboard() { throw new Error('browser clipboard accessed') } })
    mocks.read.mockResolvedValue('复制内容')
    expect(await readClipboardText()).toBe('复制内容')
    await writeClipboardText('粘贴内容')
    expect(mocks.write).toHaveBeenCalledWith('粘贴内容')
  })

  it('uses browser clipboard in browser preview', async () => {
    mocks.desktop = false
    const readText = vi.fn().mockResolvedValue('preview')
    const writeText = vi.fn().mockResolvedValue(undefined)
    vi.stubGlobal('navigator', { clipboard: { readText, writeText } })
    expect(await readClipboardText()).toBe('preview')
    await writeClipboardText('text')
    expect(writeText).toHaveBeenCalledWith('text')
    expect(mocks.read).not.toHaveBeenCalled()
    expect(mocks.write).not.toHaveBeenCalled()
  })
})
