import { describe, expect, it } from 'vitest'
import {
  defaultWorkspaceTab,
  routeForWorkspaceTab,
  sanitizeProjectWorkspace,
  workspaceToWire,
  workspaceTabId,
} from './workspaceTabs'

const fileTypes = {
  'data/item.cfd': [{ name: 'Item', display_name: 'Items', record_count: 2, is_singleton: false }],
  'data/settings.cfd': [{ name: 'Settings', display_name: 'Settings', record_count: 1, is_singleton: true }],
}

describe('project workspace tabs', () => {
  it('waits for a real singleton coordinate before entering record view', () => {
    const tab = defaultWorkspaceTab('data/settings.cfd', 'Settings', true)
    expect(tab).toMatchObject({ viewKind: 'record', viewId: '__default_record' })
    expect(routeForWorkspaceTab(tab)).toMatchObject({
      view: 'table',
      viewId: '__default_table',
      typeFilter: 'Settings',
    })
    expect(routeForWorkspaceTab(tab, { actual_type: 'Settings', key: 'settings' })).toMatchObject({
      view: 'record',
      coordinate: { actual_type: 'Settings', key: 'settings' },
    })
    expect(routeForWorkspaceTab({
      ...tab,
      coordinate: { actual_type: 'Settings', key: '' },
    }, { actual_type: 'Settings', key: 'settings' })).toMatchObject({
      view: 'record',
      coordinate: { actual_type: 'Settings', key: 'settings' },
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
    expect(restored?.tabs[1]).toMatchObject({ viewKind: 'record', viewId: '__default_record' })
    expect(restored?.activeTabId).toBe(itemId)
  })

  it('serializes workspace state using the backend wire names', () => {
    const tab = defaultWorkspaceTab('data/item.cfd', 'Item', false)
    expect(workspaceToWire([tab], tab.id)).toEqual({
      active_tab_id: tab.id,
      tabs: [{
        file_path: 'data/item.cfd',
        type_name: 'Item',
        view_id: '__default_table',
        view_kind: 'table',
        coordinate: null,
      }],
    })
  })

  it('serializes an empty record coordinate as null', () => {
    const tab = {
      ...defaultWorkspaceTab('data/settings.cfd', 'Settings', true),
      coordinate: { actual_type: 'Settings', key: '' },
    }
    expect(workspaceToWire([tab], tab.id).tabs[0].coordinate).toBeNull()
  })

  it('restores a dimension file tab with an empty type name', () => {
    const filePath = 'data/dimensions/language/item.csv'
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
