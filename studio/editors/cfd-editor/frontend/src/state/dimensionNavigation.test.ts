import { describe, expect, it } from 'vitest'
import type { DimensionFileRecords } from '../bindings/DimensionFileRecords'
import { selectDimensionRows } from './dimensionNavigation'

describe('维度行定位', () => {
  it('按业务文件、实际类型及字段隔离记录；单例保留全部字段', () => {
    const rows = [
      ['data/a.cfd', 'Item', 'name'], ['data/a.cfd', 'Item', 'description'],
      ['data/b.cfd', 'Item', 'name'], ['data/a.cfd', 'Weapon', 'name'],
    ].map(([owner_file_path, actual_type, field]) => ({
      owner_file_path, coordinate: { actual_type, key: 'id' }, field,
      default_value: { kind: 'string' as const, value: 'default' }, default_previews: {}, variant_previews: {}, values: {},
    }))
    const data = { revision: 1, file_path: '@dimension/language', dimension: 'language',
      display_name: '本地化', variants: ['en'], rows } as DimensionFileRecords
    expect(selectDimensionRows(data, { dimension: 'language', ownerFile: 'data/a.cfd', typeName: 'Item',
      field: 'name', singleton: false }).rows).toEqual([rows[0]])
    expect(selectDimensionRows(data, { dimension: 'language', ownerFile: 'data/a.cfd', typeName: 'Item',
      field: null, singleton: true }).rows).toEqual([rows[0], rows[1]])
  })
})
