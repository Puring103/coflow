// Mock data for UI prototype — no Tauri backend required.
import type { FileRecords } from './bindings/FileRecords'
import type { GraphData } from './bindings/GraphData'
import type { ProjectSnapshot } from './bindings/ProjectSnapshot'
import type { RecordRow } from './bindings/RecordRow'
import type { WriterCapabilities } from './bindings/WriterCapabilities'
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
  provider_id: 'cfd',
  can_edit_field: true,
  can_edit_key: true,
  can_insert_record: true,
  can_delete_record: true,
  requires_full_refresh_after_write: true,
  is_remote: false,
}

export const MOCK_PROJECT: ProjectSnapshot = {
  session_id: 1,
  project_root: '(mock project)',
  file_tree: [
    {
      name: 'data',
      path: 'data',
      is_dir: true,
      in_sources: true,
      children: [
        { name: 'item.cfd', path: 'data/item.cfd', is_dir: false, in_sources: true, children: [] },
        { name: 'npc.cfd', path: 'data/npc.cfd', is_dir: false, in_sources: true, children: [] },
      ],
    },
    { name: 'grey.cfd', path: 'grey.cfd', is_dir: false, in_sources: false, children: [] },
  ],
  diagnostics: [
    {
      severity: 'error',
      code: 'ref_missing',
      stage: 'check',
      message: 'npc.cfd: record Npc_001 references missing item ItemXxx',
      file_path: 'data/npc.cfd',
      actual_type: 'Npc',
      record_key: 'Npc_001',
      field_path: 'reward_item',
    },
    {
      severity: 'warning',
      code: 'unused_field',
      stage: 'check',
      message: 'item.cfd: field "legacy_id" is not in schema',
      file_path: 'data/item.cfd',
      actual_type: 'Item',
      record_key: 'Item_001',
      field_path: 'legacy_id',
    },
  ],
}

const strVal = stringValue
const intVal = (v: number): FieldValue => intValue(v)
const enumVal = (e: string, variant: string, i: number): FieldValue => enumValue(e, variant, i)
const refVal = refValue
const boolVal = boolValue

export const MOCK_FILE_RECORDS: Record<string, FileRecords> = {
  'data/item.cfd': {
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
        { name: 'legacy_id', value: nullValue(), annotation: null },
      ]),
      row('Item', 'Item_002', [
        { name: 'name', value: strVal('中级药水'), annotation: null },
        { name: 'icon', value: strVal('icon_potion_02'), annotation: null },
        { name: 'max_stack', value: intVal(50), annotation: null },
        { name: 'quality', value: enumVal('Quality', 'Uncommon', 1), annotation: null },
        { name: 'stackable', value: boolVal(true), annotation: null },
        { name: 'legacy_id', value: nullValue(), annotation: null },
      ]),
      row('Weapon', 'Sword_001', [
        { name: 'name', value: strVal('铁剑'), annotation: null },
        { name: 'damage', value: intVal(10), annotation: null },
        { name: 'rarity', value: enumVal('Quality', 'Common', 0), annotation: null },
        { name: 'two_handed', value: boolVal(false), annotation: null },
      ]),
    ],
  },
  'data/npc.cfd': {
    file_path: 'data/npc.cfd',
    type_names: ['Npc'],
    capabilities: MOCK_CFD_CAPS,
    records: [
      row('Npc', 'Npc_001', [
        { name: 'name', value: strVal('村民甲'), annotation: null },
        { name: 'level', value: intVal(1), annotation: null },
        { name: 'reward_item', value: refVal(''), annotation: null },
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
  },
}

export const MOCK_GRAPH: GraphData = {
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

function row(actualType: string, key: string, fields: RecordRow['fields']): RecordRow {
  return {
    coordinate: { actual_type: actualType, key },
    display_path: `${actualType}.${key}`,
    fields,
  }
}

function mockGraphNode(row: RecordRow, filePath: string, inFocusFile: boolean): GraphData['nodes'][number] {
  return {
    coordinate: row.coordinate,
    file_path: filePath,
    in_focus_file: inFocusFile,
    is_collapsed: false,
    fields: row.fields,
  }
}
