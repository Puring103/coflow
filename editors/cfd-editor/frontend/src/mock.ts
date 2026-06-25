// Mock data for UI prototype — no Tauri backend required
import type {
  ProjectSnapshot, FileRecords, RecordRow, GraphData, FieldValue, SourceCapabilities,
} from './bindings/index'

const MOCK_CFD_CAPS: SourceCapabilities = {
  provider_id: 'cfd',
  can_edit_field: true,
  can_edit_key: true,
  can_insert_record: true,
  can_delete_record: true,
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
        { name: 'npc.cfd',  path: 'data/npc.cfd',  is_dir: false, in_sources: true, children: [] },
      ],
    },
    { name: 'grey.cfd', path: 'grey.cfd', is_dir: false, in_sources: false, children: [] },
  ],
  diagnostics: [
    { severity: 'error',   code: 'ref_missing', stage: 'check', message: 'npc.cfd: record Npc_001 references missing item ItemXxx', file_path: 'data/npc.cfd', record_key: 'Npc_001', field_path: 'reward_item' },
    { severity: 'warning', code: 'unused_field', stage: 'check', message: 'item.cfd: field "legacy_id" is not in schema', file_path: 'data/item.cfd', record_key: 'Item_001', field_path: 'legacy_id' },
  ],
}

const strVal = (v: string): FieldValue => ({ kind: 'Str', v })
const intVal = (v: number): FieldValue => ({ kind: 'Int', v })
const enumVal = (e: string, variant: string, i: number): FieldValue => ({ kind: 'Enum', enum_name: e, variant, int_value: i })
const refVal = (tt: string, tk: string): FieldValue => ({ kind: 'Ref', target_type: tt, target_key: tk, target_file: null })
const nullVal = (): FieldValue => ({ kind: 'Null' })
const boolVal = (v: boolean): FieldValue => ({ kind: 'Bool', v })

export const MOCK_FILE_RECORDS: Record<string, FileRecords> = {
  'data/item.cfd': {
    file_path: 'data/item.cfd',
    type_names: ['Item', 'Weapon'],
    capabilities: MOCK_CFD_CAPS,
    records: [
      {
        key: 'Item_001', actual_type: 'Item',
        fields: [
          { name: 'name',     value: strVal('初级药水') },
          { name: 'icon',     value: strVal('icon_potion_01') },
          { name: 'max_stack',value: intVal(99) },
          { name: 'quality',  value: enumVal('Quality', 'Common', 0) },
          { name: 'stackable',value: boolVal(true) },
          { name: 'legacy_id',value: nullVal() },
        ],
      },
      {
        key: 'Item_002', actual_type: 'Item',
        fields: [
          { name: 'name',     value: strVal('中级药水') },
          { name: 'icon',     value: strVal('icon_potion_02') },
          { name: 'max_stack',value: intVal(50) },
          { name: 'quality',  value: enumVal('Quality', 'Uncommon', 1) },
          { name: 'stackable',value: boolVal(true) },
          { name: 'legacy_id',value: nullVal() },
        ],
      },
      {
        key: 'Sword_001', actual_type: 'Weapon',
        fields: [
          { name: 'name',      value: strVal('铁剑') },
          { name: 'damage',    value: intVal(10) },
          { name: 'rarity',    value: enumVal('Quality', 'Common', 0) },
          { name: 'two_handed',value: boolVal(false) },
        ],
      },
    ],
  },
  'data/npc.cfd': {
    file_path: 'data/npc.cfd',
    type_names: ['Npc'],
    capabilities: MOCK_CFD_CAPS,
    records: [
      {
        key: 'Npc_001', actual_type: 'Npc',
        fields: [
          { name: 'name',        value: strVal('村民甲') },
          { name: 'level',       value: intVal(1) },
          { name: 'reward_item', value: refVal('Item', 'ItemXxx') },
          { name: 'faction',     value: enumVal('Faction', 'Neutral', 0) },
          {
            name: 'drops', value: {
              kind: 'Array', items: [
                refVal('Item', 'Item_001'),
                refVal('Item', 'Item_002'),
              ],
            },
          },
        ],
      },
      {
        key: 'Npc_002', actual_type: 'Npc',
        fields: [
          { name: 'name',        value: strVal('铁匠') },
          { name: 'level',       value: intVal(5) },
          { name: 'reward_item', value: refVal('Item', 'Sword_001') },
          { name: 'faction',     value: enumVal('Faction', 'Friendly', 1) },
          { name: 'drops',       value: { kind: 'Array', items: [] } },
        ],
      },
    ],
  },
}

export const MOCK_GRAPH: GraphData = {
  nodes: [
    { id: 'data/npc.cfd::Npc_001', key: 'Npc_001', actual_type: 'Npc', file_path: 'data/npc.cfd', in_focus_file: true, is_collapsed: false, fields: MOCK_FILE_RECORDS['data/npc.cfd'].records[0].fields },
    { id: 'data/npc.cfd::Npc_002', key: 'Npc_002', actual_type: 'Npc', file_path: 'data/npc.cfd', in_focus_file: true, is_collapsed: false, fields: MOCK_FILE_RECORDS['data/npc.cfd'].records[1].fields },
    { id: 'data/item.cfd::Item_001', key: 'Item_001', actual_type: 'Item', file_path: 'data/item.cfd', in_focus_file: false, is_collapsed: false, fields: MOCK_FILE_RECORDS['data/item.cfd'].records[0].fields },
    { id: 'data/item.cfd::Item_002', key: 'Item_002', actual_type: 'Item', file_path: 'data/item.cfd', in_focus_file: false, is_collapsed: false, fields: MOCK_FILE_RECORDS['data/item.cfd'].records[1].fields },
    { id: 'data/item.cfd::Sword_001', key: 'Sword_001', actual_type: 'Weapon', file_path: 'data/item.cfd', in_focus_file: false, is_collapsed: false, fields: MOCK_FILE_RECORDS['data/item.cfd'].records[2].fields },
  ],
  edges: [
    { source: 'data/npc.cfd::Npc_001', target: 'data/item.cfd::Item_001', field_path: 'drops[0]' },
    { source: 'data/npc.cfd::Npc_001', target: 'data/item.cfd::Item_002', field_path: 'drops[1]' },
    { source: 'data/npc.cfd::Npc_002', target: 'data/item.cfd::Sword_001', field_path: 'reward_item' },
  ],
}

export const ALL_TYPE_NAMES = ['Item', 'Weapon', 'Npc']
