export interface VisibleRecordItem {
  id: string
  depth: number
  expandable: boolean
  expanded?: boolean
}

export type RecordItemDirection = 'ArrowUp' | 'ArrowDown' | 'ArrowLeft' | 'ArrowRight'

export type RecordItemNavigation =
  | { kind: 'select'; id: string }
  | { kind: 'toggle'; id: string }
  | { kind: 'boundary'; edge: 'before' | 'parent' }

export function moveRecordItem(
  items: readonly VisibleRecordItem[],
  currentId: string,
  direction: RecordItemDirection,
): RecordItemNavigation | null {
  const index = items.findIndex(item => item.id === currentId)
  if (index < 0) return null
  if (direction === 'ArrowUp' || direction === 'ArrowDown') {
    const next = index + (direction === 'ArrowDown' ? 1 : -1)
    return next >= 0 && next < items.length
      ? { kind: 'select', id: items[next].id }
      : direction === 'ArrowUp'
        ? { kind: 'boundary', edge: 'before' }
        : null
  }
  if (direction === 'ArrowRight') {
    if (!items[index].expandable) return null
    if (!items[index].expanded) return { kind: 'toggle', id: currentId }
    const child = items[index + 1]
    return child && child.depth > items[index].depth
      ? { kind: 'select', id: child.id }
      : null
  }
  if (items[index].expandable && items[index].expanded) {
    return { kind: 'toggle', id: currentId }
  }
  const depth = items[index].depth
  if (depth <= 0) return { kind: 'boundary', edge: 'parent' }
  for (let parent = index - 1; parent >= 0; parent -= 1) {
    if (items[parent].depth < depth) return { kind: 'select', id: items[parent].id }
  }
  return null
}
