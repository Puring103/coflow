import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { FrontendPluginBundle, FrontendPluginProjectState } from '../../../api'
import { replaceFrontendPlugins, setPluginProjectDefaults } from '../runtime'
import { emptyProjectPlugins, replaceProjectPlugins } from './projectPluginLoading'

vi.mock('../runtime', () => ({
  replaceFrontendPlugins: vi.fn().mockResolvedValue([]),
  setPluginProjectDefaults: vi.fn(),
}))

function bundle(id: string, scope: 'global' | 'project'): FrontendPluginBundle {
  return { id, name: id, description: '', version: '1', manifest_path: `/plugins/${id}/plugin.json`, source: '', scope, enabled: true }
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(replaceFrontendPlugins).mockResolvedValue([])
})

describe('project plugin scope loading', () => {
  it('replaces a failed project with built-in and global plugins and clears old project defaults', async () => {
    const global = bundle('global-tools', 'global')
    const previous = bundle('previous-project', 'project')
    await replaceProjectPlugins(Promise.resolve({
      ...emptyProjectPlugins(), plugins: [previous],
      defaults: { views: { Item: 'previous-project/view' }, presentations: {} },
    }), [global], [], () => true)

    const result = await replaceProjectPlugins(Promise.reject(new Error('permission denied')), [global], [], () => true)

    expect(result).toEqual({ projectLoaded: false, error: '加载项目插件失败：permission denied' })
    expect(setPluginProjectDefaults).toHaveBeenLastCalledWith({ views: {}, presentations: {} })
    expect(replaceFrontendPlugins).toHaveBeenLastCalledWith(expect.arrayContaining([
      expect.objectContaining({ id: 'project-search' }),
    ]), [global])
  })

  it('ignores an obsolete project failure after the new project has loaded', async () => {
    let rejectOld!: (error: Error) => void
    let current = 1
    const old = replaceProjectPlugins(new Promise<FrontendPluginProjectState>((_, reject) => { rejectOld = reject }), [], [], () => current === 1)
    current = 2
    const state = { ...emptyProjectPlugins(), plugins: [bundle('current-project', 'project')] }
    await replaceProjectPlugins(Promise.resolve(state), [], [], () => current === 2)
    rejectOld(new Error('old project failure'))

    expect(await old).toBeNull()
    expect(replaceFrontendPlugins).toHaveBeenCalledTimes(1)
    expect(replaceFrontendPlugins).toHaveBeenLastCalledWith(expect.any(Array), state.plugins)
    expect(setPluginProjectDefaults).toHaveBeenCalledTimes(1)
  })

  it('keeps loader and activation diagnostics when a project cannot be read', async () => {
    vi.mocked(replaceFrontendPlugins).mockResolvedValue(['global activation failed'])
    const result = await replaceProjectPlugins(Promise.reject(new Error('invalid JSON')), [], ['global manifest failed'], () => true)
    expect(result?.error).toContain('invalid JSON')
    expect(result?.error).toContain('global manifest failed')
    expect(result?.error).toContain('global activation failed')
  })
})
