import { describe, expect, it } from 'vitest'
import { dimensionTargetId, dimensionViewPath, suggestedDimensionTarget } from './dimensionNavigation'

const language = { name: 'language', display_name: '本地化', dir: 'dimensions/language', files: [], variants: [], fields: [] }
describe('维度文件导航', () => {
  it('保留完整项目相对路径并使用实际文件作为标签身份', () => {
    const target = suggestedDimensionTarget(language, 'data/equipment/items.cfd')!
    expect(target).toEqual({ dimension: 'language', filePath: 'dimensions/language/data/equipment/items.cfd' })
    expect(dimensionViewPath(target)).toBe('@dimension/language/dimensions/language/data/equipment/items.cfd')
    expect(dimensionTargetId(target)).toBe('["language","dimensions/language/data/equipment/items.cfd"]')
  })
  it('不同数据根中的同名文件不会合并', () => {
    expect(suggestedDimensionTarget(language, 'data/items.cfd')).not.toEqual(suggestedDimensionTarget(language, 'other/items.cfd'))
  })
  it('缺少目录或主体在项目外时不构造自动创建路径', () => {
    expect(suggestedDimensionTarget({ ...language, dir: null }, 'data/items.cfd')).toBeNull()
    expect(suggestedDimensionTarget(language, '../shared/items.cfd')).toBeNull()
    expect(suggestedDimensionTarget(language, 'C:/shared/items.cfd')).toBeNull()
  })
})
