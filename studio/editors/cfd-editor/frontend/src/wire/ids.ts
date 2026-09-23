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
