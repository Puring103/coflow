// Mock data for UI prototype — no Tauri backend required.
import type { FileRecords } from './bindings/FileRecords'
import type { GraphData } from './bindings/GraphData'
import type { ProjectBootstrap } from './bindings/ProjectBootstrap'
import type { EditorProjectSettings } from './bindings/EditorProjectSettings'
import type { RecordRow } from './bindings/RecordRow'
import type { WriterCapabilities } from './bindings/WriterCapabilities'
import type { DimensionFileRecords } from './api'
import { summaryOf } from './value/fieldValue'
import {
  boolValue,
  enumValue,
  intValue,
  nullValue,
  refValue,
  stringValue,
  type FieldValue,
} from './wire'

const MOCK_CFD_CAPS: WriterCapabilities = {
  can_edit_field: true,
  can_edit_key: true,
  can_insert_record: true,
  can_delete_record: true,
  can_reorder_records: true,
  requires_full_refresh_after_write: true,
}

export const MOCK_PROJECT: ProjectBootstrap = {
  schema_revision: 1,
  session_id: 1,
  revision: 1,
  project_root: '(mock project)',
  first_source_file: 'data/item.cfd',
  file_tree: [
    {
      name: 'data',
      path: 'data',
      is_dir: true,
      in_sources: true,
      in_schema: false,
      in_data: true,
      first_source_descendant: 'data/item.cfd',
      children: [
        { name: 'item.cfd', path: 'data/item.cfd', is_dir: false, in_sources: true, in_schema: false, in_data: true, first_source_descendant: 'data/item.cfd', children: [] },
        { name: 'archive.cfd', path: 'data/archive.cfd', is_dir: false, in_sources: true, in_schema: false, in_data: true, first_source_descendant: 'data/archive.cfd', children: [] },
        { name: 'npc.cfd', path: 'data/npc.cfd', is_dir: false, in_sources: true, in_schema: false, in_data: true, first_source_descendant: 'data/npc.cfd', children: [] },
      ],
    },
    { name: 'grey.cfd', path: 'grey.cfd', is_dir: false, in_sources: false, in_schema: false, in_data: false, first_source_descendant: null, children: [] },
  ],
  dimensions: [
    { name: 'language', display_name: '本地化', variants: ['zh-CN', 'en-US'], fields: [{ source_type: 'Item', source_field: 'name', is_singleton: false }] },
    { name: 'platform', display_name: '平台', variants: ['mobile', 'desktop'], fields: [{ source_type: 'Item', source_field: 'icon', is_singleton: false }] },
  ],
  file_types: {
    'data/item.cfd': [
      { name: 'Item', display_name: 'Items', record_count: 2, is_singleton: false, dimension_fields: { language: ['name'], platform: ['icon'] } },
      { name: 'Weapon', display_name: 'Weapons', record_count: 1, is_singleton: false, dimension_fields: {} },
    ],
    'data/npc.cfd': [
      { name: 'Npc', display_name: 'Npc', record_count: 2, is_singleton: false, dimension_fields: {} },
    ],
    'data/archive.cfd': [
      { name: 'Item', display_name: 'Items', record_count: 0, is_singleton: false, dimension_fields: {} },
    ],
  },
  diagnostics: [
    {
      id: 'mock-ref-missing',
      severity: 'error',
      code: 'ref_missing',
      stage: 'check',
      message: 'npc.cfd: record Npc_001 references missing item ItemXxx',
      target: { kind: 'table_field', file_path: 'data/npc.cfd', coordinate: { actual_type: 'Npc', key: 'Npc_001' }, field_path: 'reward_item' },
      contexts: [],
    },
    {
      id: 'mock-unused-field',
      severity: 'warning',
      code: 'unused_field',
      stage: 'check',
      message: 'item.cfd: field "unknown_id" is not in schema',
      target: { kind: 'table_field', file_path: 'data/item.cfd', coordinate: { actual_type: 'Item', key: 'Item_001' }, field_path: 'unknown_id' },
      contexts: [],
    },
  ],
}

const strVal = stringValue
const intVal = (v: number): FieldValue => intValue(v)
const enumVal = (e: string, variant: string, i: number): FieldValue => enumValue(e, variant, i)
const refVal = refValue
const boolVal = boolValue

export const MOCK_FILE_RECORDS: Record<string, FileRecords> = {
  'data/archive.cfd': withColumns({
    revision: 1,
    file_path: 'data/archive.cfd',
    type_names: ['Item'],
    capabilities: MOCK_CFD_CAPS,
    records: [],
  }),
  'data/item.cfd': withColumns({
    revision: 1,
    file_path: 'data/item.cfd',
    type_names: ['Item', 'Weapon'],
    capabilities: MOCK_CFD_CAPS,
    records: [
      row('Item', 'Item_001', [
        { name: 'name', value: strVal('初级药水'), annotation: null },
        { name: 'icon', value: strVal('icon_potion_01'), annotation: null },
        { name: 'max_stack', value: intVal(99), annotation: null },
        { name: 'quality', value: enumVal('Quality', 'Common', 0), annotation: null },
        { name: 'stackable', value: boolVal(true), annotation: null },
        { name: 'unknown_id', value: nullValue(), annotation: null },
      ]),
      row('Item', 'Item_002', [
        { name: 'name', value: strVal('中级药水'), annotation: null },
        { name: 'icon', value: strVal('icon_potion_02'), annotation: null },
        { name: 'max_stack', value: intVal(50), annotation: null },
        { name: 'quality', value: enumVal('Quality', 'Uncommon', 1), annotation: null },
        { name: 'stackable', value: boolVal(true), annotation: null },
        { name: 'unknown_id', value: nullValue(), annotation: null },
      ]),
      row('Weapon', 'Sword_001', [
        { name: 'name', value: strVal('铁剑'), annotation: null },
        { name: 'damage', value: intVal(10), annotation: null },
        { name: 'rarity', value: enumVal('Quality', 'Common', 0), annotation: null },
        { name: 'two_handed', value: boolVal(false), annotation: null },
      ]),
    ],
  }),
  'data/npc.cfd': withColumns({
    revision: 1,
    file_path: 'data/npc.cfd',
    type_names: ['Npc'],
    capabilities: MOCK_CFD_CAPS,
    records: [
      row('Npc', 'Npc_001', [
        { name: 'name', value: strVal('村民甲'), annotation: null },
        { name: 'level', value: intVal(1), annotation: null },
        { name: 'reward_item', value: refVal('ItemConfig.Item_001'), annotation: null },
        { name: 'faction', value: enumVal('Faction', 'Neutral', 0), annotation: null },
        {
          name: 'drops',
          value: {
            kind: 'array',
            value: [
              refVal(''),
              refVal(''),
            ],
          },
          annotation: null,
        },
      ]),
      row('Npc', 'Npc_002', [
        { name: 'name', value: strVal('铁匠'), annotation: null },
        { name: 'level', value: intVal(5), annotation: null },
        { name: 'reward_item', value: refVal(''), annotation: null },
        { name: 'faction', value: enumVal('Faction', 'Friendly', 1), annotation: null },
        { name: 'drops', value: { kind: 'array', value: [] }, annotation: null },
      ]),
    ],
  }),
}

export const MOCK_EDITOR_SETTINGS: EditorProjectSettings = {
  graph_positions: {},
  graph_compact_modes: {},
  view_order: {},
  short_name_fields: {},
    views: {},
    default_table_column_widths: {},
    workspace: { tabs: [], active_tab_id: null },
    record_groups: {
    'data/item.cfd': {
      Item: [{
        id: 'mock-potions',
        name: '药水',
        color: null,
        records: [
          { actual_type: 'Item', key: 'Item_001' },
          { actual_type: 'Item', key: 'Item_002' },
        ],
      }],
    },
  },
}

export const MOCK_DIMENSION_FILE_RECORDS: Record<string, DimensionFileRecords> = {
  '@dimension/language': {
    revision: 1,
    file_path: '@dimension/language',
    dimension: 'language',
    display_name: '本地化',
    variants: ['zh-CN', 'en-US'],
    rows: [
      {
        coordinate: { actual_type: 'Item', key: 'Item_001' },
        field: 'name',
        owner_file_path: 'data/item.cfd',
        default_value: stringValue('初级药水'),
        default_previews: {}, variant_previews: {},
        values: {
          'zh-CN': { kind: 'value', value: stringValue('初级药水') },
          'en-US': { kind: 'value', value: stringValue('Minor Potion') },
        },
      },
      {
        coordinate: { actual_type: 'Item', key: 'Item_002' },
        field: 'name',
        owner_file_path: 'data/item.cfd',
        default_value: stringValue('中级药水'),
        default_previews: {}, variant_previews: {},
        values: {
          'zh-CN': { kind: 'value', value: stringValue('中级药水') },
          'en-US': { kind: 'missing' },
        },
      },
    ],
  },
  '@dimension/platform': {
    revision: 1,
    file_path: '@dimension/platform',
    dimension: 'platform',
    display_name: '平台',
    variants: ['mobile', 'desktop'],
    rows: [],
  },
}

export const MOCK_GRAPH: GraphData = {
  revision: 1,
  available_fields: ['drops', 'reward_item'],
  nodes: [
    mockGraphNode(MOCK_FILE_RECORDS['data/npc.cfd'].records[0], 'data/npc.cfd', true),
    mockGraphNode(MOCK_FILE_RECORDS['data/npc.cfd'].records[1], 'data/npc.cfd', true),
    mockGraphNode(MOCK_FILE_RECORDS['data/item.cfd'].records[0], 'data/item.cfd', false),
    mockGraphNode(MOCK_FILE_RECORDS['data/item.cfd'].records[1], 'data/item.cfd', false),
    mockGraphNode(MOCK_FILE_RECORDS['data/item.cfd'].records[2], 'data/item.cfd', false),
  ],
  edges: [
    {
      source: { actual_type: 'Npc', key: 'Npc_001' },
      target: { actual_type: 'Item', key: 'Item_001' },
      field_path: 'drops[0]',
    },
    {
      source: { actual_type: 'Npc', key: 'Npc_001' },
      target: { actual_type: 'Item', key: 'Item_002' },
      field_path: 'drops[1]',
    },
    {
      source: { actual_type: 'Npc', key: 'Npc_002' },
      target: { actual_type: 'Weapon', key: 'Sword_001' },
      field_path: 'reward_item',
    },
  ],
}

export const ALL_TYPE_NAMES = ['Item', 'Weapon', 'Npc']

function row(
  actualType: string,
  key: string,
  fields: Array<Omit<RecordRow['fields'][number], 'missing'>>,
): RecordRow {
  const field_index: Record<string, number> = {}
  const field_summaries: Record<string, string> = {}
  fields.forEach((field, index) => {
    field_index[field.name] = index
    field_summaries[field.name] = summaryOf(field.value)
  })
  return {
    coordinate: { actual_type: actualType, key },
    display_path: `${actualType}.${key}`,
    container_index: 0,
    container_size: 1,
    fields: fields.map(field => ({ ...field, missing: false })),
    field_index,
    field_summaries,
    formatted_previews: {},
    field_diagnostics: [],
    diagnostic_severity: null,
  }
}

function withColumns(data: Omit<FileRecords, 'columns'>): FileRecords {
  const columns = new Map<string, { name: string, type_names: Set<string>, max_summary_len: number }>()
  for (const record of data.records) {
    for (const field of record.fields) {
      const column = columns.get(field.name) ?? { name: field.name, type_names: new Set<string>(), max_summary_len: 0 }
      column.type_names.add(record.coordinate.actual_type)
      column.max_summary_len = Math.max(column.max_summary_len, record.field_summaries[field.name]?.length ?? 0)
      columns.set(field.name, column)
    }
  }
  return {
    ...data,
    columns: Array.from(columns.values()).map(column => ({
      name: column.name,
      type_names: Array.from(column.type_names),
      max_summary_len: column.max_summary_len,
    })),
  }
}

function mockGraphNode(row: RecordRow, filePath: string, inFocusFile: boolean): GraphData['nodes'][number] {
  return {
    coordinate: row.coordinate,
    file_path: filePath,
    in_focus_file: inFocusFile,
    is_collapsed: false,
    fields: row.fields,
    field_diagnostics: row.field_diagnostics,
    diagnostic_severity: row.diagnostic_severity,
  }
}
