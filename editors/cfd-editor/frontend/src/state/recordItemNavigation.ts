export interface VisibleRecordItem {
  id: string
  depth: number
  expandable: boolean
}

export type RecordItemDirection = 'ArrowUp' | 'ArrowDown' | 'ArrowLeft' | 'ArrowRight'

export type RecordItemNavigation =
  | { kind: 'select'; id: string }
  | { kind: 'toggle'; id: string }

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
      : null
  }
  if (direction === 'ArrowRight') {
    return items[index].expandable ? { kind: 'toggle', id: currentId } : null
  }
  const depth = items[index].depth
  if (depth <= 0) return null
  for (let parent = index - 1; parent >= 0; parent -= 1) {
    if (items[parent].depth < depth) return { kind: 'select', id: items[parent].id }
  }
  return null
}
