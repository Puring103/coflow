import { describe, expect, it } from 'vitest'
import type { EditorProjectSettings } from '../bindings/EditorProjectSettings'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { ViewConfig } from '../bindings/ViewConfig'
import {
  DEFAULT_RECORD_VIEW_ID,
  DEFAULT_TABLE_VIEW_ID,
  RESERVED_VIEW_ID_PREFIX,
  groupFilterPredicate,
  newViewId,
  resolveView,
  viewTabsFor,
  visibleFieldsFor,
} from './views'

const FILE = 'data/item.cfd'
const TYPE = 'Item'

function tableView(over: Partial<ViewConfig> = {}): ViewConfig {
  return {
    id: 'v-table',
    name: 'Cheap',
    kind: 'table',
    group_filter: null,
    columns: ['name', 'price'],
    column_widths: { name: 120 },
    relations: [],
    fields: [],
    ...over,
  }
}

function graphView(over: Partial<ViewConfig> = {}): ViewConfig {
  return {
    id: 'v-graph',
    name: 'Refs',
    kind: 'graph',
    group_filter: null,
    columns: [],
    column_widths: {},
    relations: ['owner'],
    fields: ['name'],
    ...over,
  }
}

function settingsWith(views: ViewConfig[]): EditorProjectSettings {
  return {
    views: { [FILE]: { [TYPE]: views } },
    default_table_column_widths: { [FILE]: { [TYPE]: { name: 200 } } },
    record_groups: {},
  }
}

describe('viewTabsFor', () => {
  it('returns only the default record view for singleton types', () => {
    const tabs = viewTabsFor(null, FILE, TYPE, true, true)
    expect(tabs).toEqual([
      { id: DEFAULT_RECORD_VIEW_ID, name: '记录', kind: 'record', isDefault: true },
    ])
  })

  it('includes record + table by default, with no default graph view', () => {
    const tabs = viewTabsFor(null, FILE, TYPE, false, true).map(t => t.id)
    expect(tabs).toEqual([DEFAULT_RECORD_VIEW_ID, DEFAULT_TABLE_VIEW_ID])
  })

  it('appends custom views in order, skipping graph views when unsupported', () => {
    const settings = settingsWith([tableView({ id: 'a' }), graphView({ id: 'b' })])
    const supported = viewTabsFor(settings, FILE, TYPE, false, true).map(t => t.id)
    expect(supported).toEqual([
      DEFAULT_RECORD_VIEW_ID,
      DEFAULT_TABLE_VIEW_ID,
      'a',
      'b',
    ])
    const unsupported = viewTabsFor(settings, FILE, TYPE, false, false).map(t => t.id)
    expect(unsupported).toEqual([DEFAULT_RECORD_VIEW_ID, DEFAULT_TABLE_VIEW_ID, 'a'])
  })
})

describe('resolveView', () => {
  it('resolves the default record view without restrictions', () => {
    expect(resolveView(null, FILE, TYPE, DEFAULT_RECORD_VIEW_ID)).toMatchObject({
      kind: 'record',
      isDefault: true,
    })
  })

  it('resolves the default table view widths from settings', () => {
    const view = resolveView(settingsWith([]), FILE, TYPE, DEFAULT_TABLE_VIEW_ID)
    expect(view).toMatchObject({ kind: 'table', isDefault: true, columnWidths: { name: 200 } })
  })

  it('resolves a custom table view with its columns and widths', () => {
    const settings = settingsWith([tableView({ id: 'a', group_filter: 'g1' })])
    const view = resolveView(settings, FILE, TYPE, 'a')
    expect(view).toMatchObject({
      id: 'a',
      kind: 'table',
      isDefault: false,
      columns: ['name', 'price'],
      columnWidths: { name: 120 },
      groupFilter: 'g1',
    })
  })

  it('resolves a custom graph view with relations and fields', () => {
    const settings = settingsWith([graphView({ id: 'b' })])
    const view = resolveView(settings, FILE, TYPE, 'b')
    expect(view).toMatchObject({
      id: 'b',
      kind: 'graph',
      isDefault: false,
      relations: ['owner'],
      fields: ['name'],
    })
  })

  it('falls back to the default table view for unknown ids', () => {
    const view = resolveView(settingsWith([]), FILE, TYPE, 'does-not-exist')
    expect(view).toMatchObject({ id: DEFAULT_TABLE_VIEW_ID, kind: 'table', isDefault: true })
  })
})

describe('visibleFieldsFor', () => {
  it('returns undefined (all fields) for the default table view', () => {
    expect(visibleFieldsFor(resolveView(null, FILE, TYPE, DEFAULT_TABLE_VIEW_ID))).toBeUndefined()
  })

  it('restricts to columns for custom table views and fields for graph views', () => {
    const table = resolveView(settingsWith([tableView({ id: 'a' })]), FILE, TYPE, 'a')
    expect(visibleFieldsFor(table)).toEqual(new Set(['name', 'price']))
    const graph = resolveView(settingsWith([graphView({ id: 'b' })]), FILE, TYPE, 'b')
    expect(visibleFieldsFor(graph)).toEqual(new Set(['name']))
  })
})

describe('groupFilterPredicate', () => {
  const groups: EditorRecordGroup[] = [
    {
      id: 'g1',
      name: 'Potions',
      color: null,
      records: [
        { actual_type: 'Item', key: 'a' },
        { actual_type: 'Item', key: 'b' },
      ],
    },
  ]

  it('passes all when there is no group filter', () => {
    const predicate = groupFilterPredicate(resolveView(null, FILE, TYPE, DEFAULT_TABLE_VIEW_ID), groups)
    expect(predicate({ actual_type: 'Item', key: 'z' })).toBe(true)
  })

  it('passes all when the referenced group no longer exists', () => {
    const settings = settingsWith([tableView({ id: 'a', group_filter: 'missing' })])
    const predicate = groupFilterPredicate(resolveView(settings, FILE, TYPE, 'a'), groups)
    expect(predicate({ actual_type: 'Item', key: 'a' })).toBe(true)
  })

  it('restricts to group members when the filter is valid', () => {
    const settings = settingsWith([tableView({ id: 'a', group_filter: 'g1' })])
    const predicate = groupFilterPredicate(resolveView(settings, FILE, TYPE, 'a'), groups)
    expect(predicate({ actual_type: 'Item', key: 'a' })).toBe(true)
    expect(predicate({ actual_type: 'Item', key: 'z' })).toBe(false)
  })
})

describe('newViewId', () => {
  it('never collides with the reserved prefix and is unique', () => {
    const a = newViewId()
    const b = newViewId()
    expect(a.startsWith(RESERVED_VIEW_ID_PREFIX)).toBe(false)
    expect(a).not.toBe(b)
  })
})
