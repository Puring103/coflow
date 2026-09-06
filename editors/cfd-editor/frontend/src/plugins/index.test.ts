import { afterEach, describe, expect, it, vi } from 'vitest'
import type { FrontendPluginBundle } from '../api'
import {
  activateFrontendPlugin,
  dispatchPluginKeybinding,
  executePluginCommand,
  failPluginContribution,
  pluginRegistrySnapshot,
  preferredPluginView,
  publishPluginEvent,
  replaceFrontendPlugins,
  resetPluginRegistryForTests,
  resolvePluginPresentation,
  setPluginDataBridge,
  setFrontendPluginEnabled,
  setPluginProjectDefaults,
  setPluginUiBridge,
  unloadFrontendPlugin,
} from './index'
import type { EditorPluginHost } from './types'
import { projectSearchPlugin } from '../built-in-plugins/projectSearch'

function bundle(id: string): FrontendPluginBundle {
  return {
    id,
    name: id,
    description: '',
    version: '1',
    manifest_path: `/plugins/${id}/plugin.json`,
    source: '',
    scope: 'project',
    enabled: true,
  }
}

afterEach(() => resetPluginRegistryForTests())

describe('editor plugin registry', () => {
  it('rejects invalid plugin ids', async () => {
    await expect(activateFrontendPlugin(bundle('invalid/id'), () => {}))
      .rejects.toThrow('插件 id')
  })

  it('loads built-in plugins through the registry and keeps them enabled', async () => {
    await replaceFrontendPlugins([projectSearchPlugin], [])

    expect(pluginRegistrySnapshot().plugins).toEqual([
      expect.objectContaining({ id: 'project-search', origin: 'built-in', enabled: true }),
    ])
    expect(pluginRegistrySnapshot().sidebars).toEqual([
      expect.objectContaining({ key: 'project-search/project-search', icon: 'search' }),
    ])

    setFrontendPluginEnabled('project-search', false)
    expect(pluginRegistrySnapshot().plugins[0].enabled).toBe(true)
  })

  it('opens a sidebar through a plugin command', async () => {
    const openSidebar = vi.fn()
    setPluginUiBridge({
      openPage() {},
      closePage() {},
      openSidebar,
      closeSidebar() {},
      openRecord() {},
      reportError() {},
    })
    await activateFrontendPlugin(bundle('navigation'), host => {
      host.register.sidebar({ id: 'search', title: 'Search', mount() {} })
      host.register.command({
        id: 'open',
        title: 'Open',
        run() { host.register.openSidebar('search') },
      })
    })

    const command = pluginRegistrySnapshot().commands[0]
    await executePluginCommand(command.key, {
      identity: null,
      filePath: null,
      typeName: null,
      selection: null,
      surface: 'editor',
    })

    expect(openSidebar).toHaveBeenCalledWith('navigation', 'search')
  })

  it('rejects empty and wildcard type targets', async () => {
    await expect(activateFrontendPlugin(bundle('empty'), host => {
      host.register.view({ id: 'view', title: 'View', types: [], mount() {} })
    })).rejects.toThrow('明确类型列表')

    await expect(activateFrontendPlugin(bundle('wildcard'), host => {
      host.register.presentation({ id: 'cell', slot: 'cell', types: ['*'], mount() {} })
    })).rejects.toThrow('明确类型列表')
  })

  it('keeps every matching selectable view in stable load order', async () => {
    await activateFrontendPlugin(bundle('first'), host => {
      host.register.view({ id: 'one', title: 'One', types: ['Foo'], mount() {} })
    })
    await activateFrontendPlugin(bundle('second'), host => {
      host.register.view({ id: 'two', title: 'Two', types: ['Foo'], mount() {} })
    })

    expect(pluginRegistrySnapshot().views.map(view => view.key)).toEqual([
      'first/one',
      'second/two',
    ])
  })

  it('publishes contributions registered and removed after activation', async () => {
    let host: EditorPluginHost | undefined
    await activateFrontendPlugin(bundle('dynamic'), current => { host = current })

    const dispose = host!.register.page({ id: 'report', title: 'Report', mount() {} })
    expect(pluginRegistrySnapshot().pages.map(page => page.key)).toEqual(['dynamic/report'])

    dispose()
    expect(pluginRegistrySnapshot().pages).toHaveLength(0)
  })

  it('uses project defaults before registration defaults and first match', async () => {
    await activateFrontendPlugin(bundle('first'), host => {
      host.register.presentation({ id: 'summary', slot: 'summary', types: ['Foo'], render: () => 'first' })
      host.register.view({ id: 'view', title: 'First', types: ['Foo'], mount() {} })
    })
    await activateFrontendPlugin(bundle('second'), host => {
      host.register.presentation({ id: 'summary', slot: 'summary', types: ['Foo'], default: true, render: () => 'second' })
      host.register.view({ id: 'view', title: 'Second', types: ['Foo'], default: true, mount() {} })
    })

    expect(resolvePluginPresentation(pluginRegistrySnapshot(), 'summary', 'Foo')?.key).toBe('second/summary')
    expect(preferredPluginView('Foo')?.key).toBe('second/view')

    setPluginProjectDefaults({
      views: { Foo: 'first/view' },
      presentations: { Foo: { summary: 'first/summary' } },
    })
    expect(resolvePluginPresentation(pluginRegistrySnapshot(), 'summary', 'Foo')?.key).toBe('first/summary')
    expect(preferredPluginView('Foo')?.key).toBe('first/view')
  })

  it('isolates a failed contribution without removing sibling contributions', async () => {
    await activateFrontendPlugin(bundle('sample'), host => {
      host.register.page({ id: 'page', title: 'Page', mount() {} })
      host.register.sidebar({ id: 'sidebar', title: 'Sidebar', mount() {} })
    })

    failPluginContribution('sample/page', new Error('failed'))
    expect(pluginRegistrySnapshot().pages).toHaveLength(0)
    expect(pluginRegistrySnapshot().sidebars.map(sidebar => sidebar.key)).toEqual(['sample/sidebar'])
  })

  it('allows a failed contribution id to be registered again after disposal', async () => {
    let host: EditorPluginHost | undefined
    let disposePage: (() => void) | undefined
    await activateFrontendPlugin(bundle('sample'), current => {
      host = current
      disposePage = current.register.page({ id: 'page', title: 'Page', mount() {} })
    })
    failPluginContribution('sample/page', new Error('failed'))

    disposePage!()
    host!.register.page({ id: 'page', title: 'Recovered', mount() {} })

    expect(pluginRegistrySnapshot().pages.map(page => page.title)).toEqual(['Recovered'])
  })

  it('delivers data revision events and removes subscriptions on unload', async () => {
    const listener = vi.fn()
    await activateFrontendPlugin(bundle('sample'), host => {
      host.events.on('data', listener)
    })

    publishPluginEvent('data', { sessionId: 3, revision: 7 })
    expect(listener).toHaveBeenCalledWith({ sessionId: 3, revision: 7 })
    unloadFrontendPlugin('sample')
    publishPluginEvent('data', { sessionId: 3, revision: 8 })
    expect(listener).toHaveBeenCalledOnce()
  })

  it('isolates rejected event handlers and continues dispatching', async () => {
    const reportError = vi.fn()
    const second = vi.fn()
    setPluginUiBridge({
      openPage() {},
      closePage() {},
      openSidebar() {},
      closeSidebar() {},
      openRecord() {},
      reportError,
    })
    await activateFrontendPlugin(bundle('events'), host => {
      host.events.on('data', async () => { throw new Error('listener failed') })
      host.events.on('data', second)
    })

    publishPluginEvent('data', { sessionId: 3, revision: 7 })
    await Promise.resolve()
    await Promise.resolve()

    expect(second).toHaveBeenCalledOnce()
    expect(reportError).toHaveBeenCalledWith(expect.stringContaining('listener failed'))
  })

  it('dispatches the first matching keybinding through its registered command', async () => {
    const run = vi.fn()
    await activateFrontendPlugin(bundle('commands'), host => {
      host.register.command({ id: 'refresh', title: 'Refresh', run })
      host.register.keybinding({ command: 'refresh', key: 'Mod+Shift+R' })
    })
    const preventDefault = vi.fn()
    const handled = dispatchPluginKeybinding({
      key: 'r',
      ctrlKey: true,
      metaKey: false,
      altKey: false,
      shiftKey: true,
      defaultPrevented: false,
      isComposing: false,
      preventDefault,
    } as unknown as KeyboardEvent, {
      identity: null,
      filePath: null,
      typeName: null,
      selection: null,
      surface: 'editor',
    })
    await Promise.resolve()

    expect(handled).toBe(true)
    expect(preventDefault).toHaveBeenCalledOnce()
    expect(run).toHaveBeenCalledOnce()
  })

  it('does not reuse internal keybinding ids after disposal', async () => {
    let host: EditorPluginHost | undefined
    let disposeFirst: (() => void) | undefined
    await activateFrontendPlugin(bundle('bindings'), current => {
      host = current
      current.register.command({ id: 'keybinding-0', title: 'Public command', run() {} })
      disposeFirst = current.register.keybinding({ command: 'keybinding-0', key: 'Ctrl+1' })
      current.register.keybinding({ command: 'keybinding-0', key: 'Ctrl+2' })
    })

    disposeFirst!()
    host!.register.keybinding({ command: 'keybinding-0', key: 'Ctrl+3' })

    expect(pluginRegistrySnapshot().keybindings.map(binding => binding.key)).toEqual([
      'bindings/@keybinding-1',
      'bindings/@keybinding-2',
    ])
  })

  it('does not activate disabled plugins and disposes them when disabled', async () => {
    const disabledActivate = vi.fn()
    await activateFrontendPlugin({ ...bundle('disabled'), enabled: false }, disabledActivate)

    expect(disabledActivate).not.toHaveBeenCalled()
    expect(pluginRegistrySnapshot().plugins).toEqual([
      expect.objectContaining({ id: 'disabled', enabled: false }),
    ])

    const dispose = vi.fn()
    const listener = vi.fn()
    await activateFrontendPlugin(bundle('active'), host => {
      host.register.page({ id: 'page', title: 'Page', mount() {} })
      host.events.on('data', listener)
      return { dispose }
    })

    setFrontendPluginEnabled('active', false)
    publishPluginEvent('data', { sessionId: 3, revision: 7 })

    expect(dispose).toHaveBeenCalledOnce()
    expect(listener).not.toHaveBeenCalled()
    expect(pluginRegistrySnapshot().pages).toHaveLength(0)
    expect(pluginRegistrySnapshot().plugins.find(plugin => plugin.id === 'active')?.enabled).toBe(false)
  })

  it('delegates one mutation request and returns the published revision', async () => {
    let identity = { sessionId: 5, revision: 9 }
    const mutate = vi.fn(async () => { identity = { sessionId: 5, revision: 10 } })
    setPluginDataBridge({
      currentIdentity: () => identity,
      getSchema: async () => [],
      getRecordsByType: async () => [],
      getFileRecords: async () => ({
        revision: identity.revision,
        file_path: 'data.cfd',
        type_names: [],
        records: [],
        columns: [],
        capabilities: {
          can_edit_field: true,
          can_edit_key: true,
          can_insert_record: true,
          can_delete_record: true,
          can_reorder_records: true,
          requires_full_refresh_after_write: false,
        },
      }),
      searchRecords: async () => { throw new Error('not used') },
      mutate,
    })
    let result: { sessionId: number; revision: number } | undefined
    await activateFrontendPlugin(bundle('writer'), async host => {
      result = await host.data.mutate({
        kind: 'delete_record',
        filePath: 'data.cfd',
        coordinate: { actual_type: 'Foo', key: 'one' },
      })
    })

    expect(mutate).toHaveBeenCalledOnce()
    expect(result).toEqual({ sessionId: 5, revision: 10 })
  })

  it('serializes concurrent plugin mutations as separate revisions', async () => {
    let identity = { sessionId: 5, revision: 9 }
    const resolvers: Array<() => void> = []
    const mutate = vi.fn(() => new Promise<void>(resolve => {
      resolvers.push(() => {
        identity = { ...identity, revision: identity.revision + 1 }
        resolve()
      })
    }))
    let host: EditorPluginHost | undefined
    setPluginDataBridge({
      currentIdentity: () => identity,
      getSchema: async () => [],
      getRecordsByType: async () => [],
      getFileRecords: async () => { throw new Error('not used') },
      searchRecords: async () => { throw new Error('not used') },
      mutate,
    })
    await activateFrontendPlugin(bundle('writer'), current => { host = current })
    const request = {
      kind: 'delete_record' as const,
      filePath: 'data.cfd',
      coordinate: { actual_type: 'Foo', key: 'one' },
    }

    const first = host!.data.mutate(request)
    const second = host!.data.mutate(request)
    await Promise.resolve()
    expect(mutate).toHaveBeenCalledOnce()

    resolvers.shift()!()
    await expect(first).resolves.toEqual({ sessionId: 5, revision: 10 })
    await Promise.resolve()
    expect(mutate).toHaveBeenCalledTimes(2)

    resolvers.shift()!()
    await expect(second).resolves.toEqual({ sessionId: 5, revision: 11 })
  })

  it('omits field values when a record query requests identifiers only', async () => {
    let host: EditorPluginHost | undefined
    setPluginDataBridge({
      currentIdentity: () => ({ sessionId: 5, revision: 9 }),
      getSchema: async () => [],
      getRecordsByType: async () => [{
        coordinate: { actual_type: 'Foo', key: 'one' },
        display_path: 'data.cfd',
        fields: [{ name: 'value', value: { kind: 'string', value: 'secret' } }],
      } as never],
      getFileRecords: async () => { throw new Error('not used') },
      searchRecords: async () => { throw new Error('not used') },
      mutate: async () => {},
    })
    await activateFrontendPlugin(bundle('reader'), current => { host = current })

    const result = await host!.data.getRecordsByType('Foo', { includeFieldValues: false })

    expect(result).toEqual({
      sessionId: 5,
      revision: 9,
      data: [{
        filePath: 'data.cfd',
        coordinate: { actual_type: 'Foo', key: 'one' },
      }],
    })
  })

  it('normalizes a search query and returns the captured revision', async () => {
    let host: EditorPluginHost | undefined
    const searchRecords = vi.fn(async (sessionId: number, query: string) => ({
      sessionId,
      revision: 9,
      data: {
        hits: [{
          filePath: 'data.cfd',
          coordinate: { actual_type: 'Foo', key: 'one' },
          fieldPath: null,
          preview: null,
        }],
        truncated: false,
      },
    }))
    setPluginDataBridge({
      currentIdentity: () => ({ sessionId: 5, revision: 9 }),
      getSchema: async () => [],
      getRecordsByType: async () => [],
      getFileRecords: async () => { throw new Error('not used') },
      searchRecords,
      mutate: async () => {},
    })
    await activateFrontendPlugin(bundle('searcher'), current => { host = current })

    const result = await host!.data.searchRecords('  one  ')

    expect(searchRecords).toHaveBeenCalledWith(5, 'one', 'key', 200)
    expect(result.data.hits[0].coordinate).toEqual({ actual_type: 'Foo', key: 'one' })
  })

  it('rejects a query superseded by a newer data revision', async () => {
    let identity = { sessionId: 5, revision: 9 }
    let resolveSchema: (() => void) | undefined
    let host: EditorPluginHost | undefined
    setPluginDataBridge({
      currentIdentity: () => identity,
      getSchema: () => new Promise(resolve => { resolveSchema = () => resolve([]) }),
      getRecordsByType: async () => [],
      getFileRecords: async () => { throw new Error('not used') },
      searchRecords: async () => { throw new Error('not used') },
      mutate: async () => {},
    })
    await activateFrontendPlugin(bundle('reader'), current => { host = current })

    const pending = host!.data.getSchema()
    identity = { sessionId: 5, revision: 10 }
    resolveSchema!()

    await expect(pending).rejects.toThrow('修订已过期')
  })

  it('disposes the plugin definition when unloaded', async () => {
    const dispose = vi.fn()
    await activateFrontendPlugin(bundle('lifecycle'), () => ({ dispose }))

    unloadFrontendPlugin('lifecycle')

    expect(dispose).toHaveBeenCalledOnce()
  })
})
