import { optionLayerStates, presentationValue, type FieldValue } from '../wire'
import { collectionShapeForDeclaredType, optionDepthForDeclaredType } from './fieldValue'

/** 行渲染与几何共用 Option 可达性，避免为空的外层 Option 生成内部行。 */
export function fieldRowPresentation(value: FieldValue, declaredType?: string) {
  const shownValue = presentationValue(value)
  const depth = optionDepthForDeclaredType(declaredType)
  const none = optionLayerStates(value, depth).indexOf('none')
  const innerValueAvailable = none < 0 || none === depth - 1
  const emptyCollection = shownValue.kind === 'option_none' && innerValueAvailable
    ? collectionShapeForDeclaredType(declaredType) : null
  const displayValue = emptyCollection ?? shownValue
  const canExpand = displayValue.kind === 'object' || displayValue.kind === 'array' || displayValue.kind === 'dict'
  return { shownValue, displayValue, innerValueAvailable, canExpand }
}
