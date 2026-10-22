import { describe, expect, it } from 'vitest'
import {
  defaultWorkspaceTab,
  dimensionWorkspaceTab,
  routeForWorkspaceTab,
  sanitizeProjectWorkspace,
  workspaceTabWithView,
  workspaceToWire,
  workspaceTabId,
} from './workspaceTabs'

const fileTypes = {
  'data/item.cfd': [{ name: 'Item', display_name: 'Items', record_count: 2, is_singleton: false, dimension_fields: {} }],
  'data/settings.cfd': [{ name: 'Settings', display_name: 'Settings', record_count: 1, is_singleton: true, dimension_fields: {} }],
}

describe('project workspace tabs', () => {
  it('opens CFT files directly in their only source view', () => {
    const tab = defaultWorkspaceTab('schema/items.cft', '')
    expect(tab).toMatchObject({ viewKind: 'source', viewId: '__default_source' })
    expect(routeForWorkspaceTab(tab)).toMatchObject({
      view: 'source',
      file: 'schema/items.cft',
      typeFilter: '',
    })
  })

  it('defaults singleton files to table and resolves coordinates only for an explicit record view', () => {
    const tab = defaultWorkspaceTab('data/settings.cfd', 'Settings')
    expect(tab).toMatchObject({ viewKind: 'table', viewId: '__default_table' })
    expect(routeForWorkspaceTab(tab, { actual_type: 'Settings', id: 'settings' })).toMatchObject({
      view: 'table', viewId: '__default_table', typeFilter: 'Settings',
    })
    const recordTab = workspaceTabWithView(tab, 'record', '__default_record')
    expect(routeForWorkspaceTab(recordTab)).toMatchObject({ view: 'table', viewId: '__default_table' })
    expect(routeForWorkspaceTab(recordTab, { actual_type: 'Settings', id: 'settings' })).toMatchObject({
      view: 'record', coordinate: { actual_type: 'Settings', id: 'settings' },
    })
  })

  it('restores valid tabs and their independent views', () => {
    const itemId = workspaceTabId('data/item.cfd', 'Item')
    const restored = sanitizeProjectWorkspace({
      active_tab_id: itemId,
      tabs: [
        { file_path: 'data/item.cfd', type_name: 'Item', view_kind: 'graph', view_id: 'refs' },
        { file_path: 'data/settings.cfd', type_name: 'Settings', view_kind: 'table', view_id: '__default_table' },
        { file_path: 'missing.cfd', type_name: 'Missing', view_kind: 'table', view_id: '__default_table' },
      ],
    }, fileTypes)
    expect(restored?.tabs).toHaveLength(2)
    expect(restored?.tabs[0]).toMatchObject({ viewKind: 'graph', viewId: 'refs' })
    expect(restored?.tabs[1]).toMatchObject({ viewKind: 'table', viewId: '__default_table' })
    expect(restored?.activeTabId).toBe(itemId)
  })

  it('restores source view for singleton files', () => {
    const restored = sanitizeProjectWorkspace({
      active_tab_id: workspaceTabId('data/settings.cfd', 'Settings'),
      tabs: [{
        file_path: 'data/settings.cfd',
        type_name: 'Settings',
        view_kind: 'source',
        view_id: '__default_source',
      }],
    }, fileTypes)
    const tab = restored?.tabs[0]
    expect(tab).toMatchObject({ viewKind: 'source', viewId: '__default_source' })
    expect(routeForWorkspaceTab(tab!)).toMatchObject({ view: 'source', viewId: '__default_source' })
  })

  it('preserves a plugin view for a singleton type until plugins load', () => {
    const restored = sanitizeProjectWorkspace({
      active_tab_id: workspaceTabId('data/settings.cfd', 'Settings'),
      tabs: [{
        file_path: 'data/settings.cfd',
        type_name: 'Settings',
        view_kind: 'table',
        view_id: 'analysis/summary',
      }],
    }, fileTypes)

    expect(restored?.tabs[0]).toMatchObject({
      viewKind: 'table',
      viewId: 'analysis/summary',
    })
    expect(routeForWorkspaceTab(restored!.tabs[0])).toMatchObject({
      view: 'table',
      viewId: 'analysis/summary',
      typeFilter: 'Settings',
    })
  })

  it('switches a record workspace tab back to source before routing', () => {
    const record = {
      ...defaultWorkspaceTab('data/item.cfd', 'Item'),
      viewKind: 'record' as const,
      viewId: '__default_record',
      coordinate: { actual_type: 'Item', id: 'sword' },
    }
    const source = workspaceTabWithView(record, 'source', '__default_source')

    expect(source).toMatchObject({ viewKind: 'source', viewId: '__default_source' })
    expect(routeForWorkspaceTab(source)).toMatchObject({
      view: 'source',
      viewId: '__default_source',
      typeFilter: 'Item',
    })
  })

  it('serializes workspace state using the backend wire names', () => {
    const tab = defaultWorkspaceTab('data/item.cfd', 'Item')
    expect(workspaceToWire([tab], tab.id)).toEqual({
      active_tab_id: tab.id,
      tabs: [{
        file_path: 'data/item.cfd',
        type_name: 'Item',
        view_id: '__default_table',
        view_kind: 'table',
        coordinate: null,
        dimension_target: null,
      }],
    })
  })

  it('serializes an empty record coordinate as null', () => {
    const tab = {
      ...defaultWorkspaceTab('data/settings.cfd', 'Settings'),
      coordinate: { actual_type: 'Settings', id: '' },
    }
    expect(workspaceToWire([tab], tab.id).tabs[0].coordinate).toBeNull()
  })

  it('restores a dimension file tab with an empty type name', () => {
    const filePath = 'data/dimensions/language/item.cfd'
    const id = workspaceTabId(filePath, '')
    const restored = sanitizeProjectWorkspace({
      active_tab_id: id,
      tabs: [{
        file_path: filePath,
        type_name: '',
        view_kind: 'record',
        view_id: '__default_record',
      }],
    }, fileTypes, new Set([filePath]))
    expect(restored?.tabs[0]).toMatchObject({
      id,
      typeName: '',
      viewKind: 'record',
    })
  })
})


describe('维度工作区', () => {
  it('按实际文件恢复标签，也保留配置目录内尚未写入的文件入口', () => {
    const existing = dimensionWorkspaceTab({ dimension: 'language', filePath: 'scattered/ja.cfd' })
    const pending = dimensionWorkspaceTab({ dimension: 'language', filePath: 'dimensions/language/data/items.cfd' })
    const wire = workspaceToWire([existing, pending], pending.id)
    const restored = sanitizeProjectWorkspace(wire, {}, new Set(), [{
      name: 'language', display_name: '本地化', variants: ['en'], fields: [],
      dir: 'dimensions/language', files: ['scattered/ja.cfd'],
    }])
    expect(restored?.tabs.map(tab => tab.id)).toEqual([existing.id, pending.id])
    expect(restored?.activeTabId).toBe(pending.id)
    expect(restored?.tabs[1]?.dimensionTarget).toEqual(pending.dimensionTarget)
    expect(routeForWorkspaceTab(existing).file).toBe('@dimension/language/scattered/ja.cfd')
  })
})
