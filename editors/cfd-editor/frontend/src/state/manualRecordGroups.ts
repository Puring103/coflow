import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import { coordinateId, sameCoordinate } from '../wire'

export interface RecordGroupView {
  group: EditorRecordGroup
  records: RecordRow[]
}

export interface GroupedRecordRows {
  groups: RecordGroupView[]
  ungrouped: RecordRow[]
}

export function organizeRecordRows(
  records: readonly RecordRow[],
  groups: readonly EditorRecordGroup[],
): GroupedRecordRows {
  const groupByRecord = new Map<string, string>()
  for (const group of groups) {
    for (const coordinate of group.records) {
      const id = coordinateId(coordinate)
      if (!groupByRecord.has(id)) groupByRecord.set(id, group.id)
    }
  }
  const recordsByGroup = new Map<string, RecordRow[]>()
  const ungrouped: RecordRow[] = []
  for (const record of records) {
    const id = coordinateId(record.coordinate)
    const groupId = groupByRecord.get(id)
    if (!groupId) {
      ungrouped.push(record)
      continue
    }
    const members = recordsByGroup.get(groupId) ?? []
    members.push(record)
    recordsByGroup.set(groupId, members)
  }

  return {
    groups: groups.flatMap(group => {
      const members = recordsByGroup.get(group.id) ?? []
      return members.length > 0 ? [{ group, records: members }] : []
    }),
    ungrouped,
  }
}

export function moveRecordOntoRecord(
  groups: readonly EditorRecordGroup[],
  source: RecordCoordinate,
  target: RecordCoordinate,
  newGroupId: string,
  newGroupName: string,
): EditorRecordGroup[] {
  if (sameCoordinate(source, target)) return [...groups]
  const targetGroup = groupContaining(groups, target)
  if (targetGroup) return moveRecordToGroup(groups, source, targetGroup.id)

  return [
    ...removeRecordFromGroups(groups, source),
    { id: newGroupId, name: newGroupName, records: [target, source] },
  ]
}

export function moveRecordToGroup(
  groups: readonly EditorRecordGroup[],
  source: RecordCoordinate,
  targetGroupId: string,
): EditorRecordGroup[] {
  if (groupContaining(groups, source)?.id === targetGroupId) return [...groups]
  const withoutSource = removeRecordFromGroups(groups, source)
  return withoutSource.map(group => group.id === targetGroupId
    ? { ...group, records: [...group.records, source] }
    : group)
}

export function removeRecordFromGroups(
  groups: readonly EditorRecordGroup[],
  coordinate: RecordCoordinate,
): EditorRecordGroup[] {
  return groups.flatMap(group => {
    const records = group.records.filter(member => !sameCoordinate(member, coordinate))
    return records.length >= 2 ? [{ ...group, records }] : []
  })
}

export function renameRecordGroup(
  groups: readonly EditorRecordGroup[],
  groupId: string,
  name: string,
): EditorRecordGroup[] {
  const trimmed = name.trim().slice(0, 80)
  if (!trimmed) return [...groups]
  return groups.map(group => group.id === groupId ? { ...group, name: trimmed } : group)
}

export function replaceGroupedCoordinate(
  groups: readonly EditorRecordGroup[],
  previous: RecordCoordinate,
  next: RecordCoordinate,
): EditorRecordGroup[] {
  return groups.map(group => ({
    ...group,
    records: group.records.map(member => sameCoordinate(member, previous) ? next : member),
  }))
}

export function nextRecordGroupName(groups: readonly EditorRecordGroup[]): string {
  const names = new Set(groups.map(group => group.name))
  if (!names.has('新分组')) return '新分组'
  let suffix = 2
  while (names.has(`新分组 ${suffix}`)) suffix += 1
  return `新分组 ${suffix}`
}

function groupContaining(
  groups: readonly EditorRecordGroup[],
  coordinate: RecordCoordinate,
): EditorRecordGroup | undefined {
  return groups.find(group => group.records.some(member => sameCoordinate(member, coordinate)))
}
