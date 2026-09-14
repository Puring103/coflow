import type { RecordCoordinate } from '../bindings/RecordCoordinate'

/** 坐标/记录身份：全局唯一的坐标键，查询键与写入键统一经此处构造。 */
export function coordinateId(coordinate: Pick<RecordCoordinate, 'actual_type' | 'key'>): string {
  return `${encodeURIComponent(coordinate.actual_type)}::${encodeURIComponent(coordinate.key)}`
}

export function sameCoordinate(
  a: Pick<RecordCoordinate, 'actual_type' | 'key'>,
  b: Pick<RecordCoordinate, 'actual_type' | 'key'>,
): boolean {
  return a.actual_type === b.actual_type && a.key === b.key
}

/** 字段写入键：与后端会话内写入目标一一对应，分隔符统一收敛于此。 */
export function fieldWriteKey(coordinate: Pick<RecordCoordinate, 'actual_type' | 'key'>, fieldPath: string): string {
  return `${coordinateId(coordinate)}\u001ffield:${fieldPath}`
}

/** 维度写入键：与字段写入键同一分隔约定。 */
export function dimensionWriteKey(coordinate: Pick<RecordCoordinate, 'actual_type' | 'key'>, field: string): string {
  return `${coordinateId(coordinate)}\u001fdimension:${field}`
}
