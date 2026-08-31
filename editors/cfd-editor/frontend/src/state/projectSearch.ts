import type { FileRecords } from '../bindings/FileRecords'
import type { ProjectSearchHit } from '../bindings/ProjectSearchHit'
import type { ProjectSearchMode } from '../bindings/ProjectSearchMode'
import type { ProjectSearchResults } from '../bindings/ProjectSearchResults'
import type { RecordRow } from '../bindings/RecordRow'
import type { FieldValue } from '../wire'
import { summaryOf } from '../value/fieldValue'

export type ProjectSearchTypeGroup = {
  actualType: string
  hits: ProjectSearchHit[]
}

export type ProjectSearchFileGroup = {
  filePath: string
  hits: ProjectSearchHit[]
  types: ProjectSearchTypeGroup[]
}

export function groupProjectSearchHits(hits: readonly ProjectSearchHit[]): ProjectSearchFileGroup[] {
  const files = new Map<string, Map<string, ProjectSearchHit[]>>()
  for (const hit of hits) {
    const types = files.get(hit.file_path) ?? new Map<string, ProjectSearchHit[]>()
    if (!files.has(hit.file_path)) files.set(hit.file_path, types)
    const typeHits = types.get(hit.coordinate.actual_type) ?? []
    if (!types.has(hit.coordinate.actual_type)) types.set(hit.coordinate.actual_type, typeHits)
    typeHits.push(hit)
  }
  return Array.from(files, ([filePath, types]) => {
    const typeGroups = Array.from(types, ([actualType, typeHits]) => ({ actualType, hits: typeHits }))
    return { filePath, hits: typeGroups.flatMap(group => group.hits), types: typeGroups }
  })
}

export function searchMockRecords(
  files: Record<string, FileRecords>,
  revision: number,
  query: string,
  mode: ProjectSearchMode,
  limit: number,
): ProjectSearchResults {
  const normalized = query.trim().toLowerCase()
  if (!normalized || limit <= 0) return { revision, hits: [], truncated: false }
  const hits: ProjectSearchHit[] = []
  for (const filePath of Object.keys(files).sort()) {
    for (const record of files[filePath].records) {
      const keyMatches = record.coordinate.key.toLowerCase().includes(normalized)
      const fieldMatch = mode === 'full_text' && !keyMatches
        ? firstFieldMatch(record, normalized)
        : null
      if (!keyMatches && !fieldMatch) continue
      if (hits.length === limit) return { revision, hits, truncated: true }
      hits.push({
        file_path: filePath,
        coordinate: record.coordinate,
        field_path: fieldMatch?.fieldPath ?? null,
        preview: fieldMatch?.preview ?? null,
      })
    }
  }
  return { revision, hits, truncated: false }
}

function firstFieldMatch(record: RecordRow, query: string): { fieldPath: string, preview: string } | null {
  for (const field of record.fields) {
    if (field.name.toLowerCase().includes(query)) {
      return { fieldPath: field.name, preview: `${field.name}: ${summaryOf(field.value)}` }
    }
    const match = valueMatch(field.value, query, field.name)
    if (match) return match
  }
  return null
}

function valueMatch(value: FieldValue, query: string, path: string): { fieldPath: string, preview: string } | null {
  const scalar = scalarSearchText(value)
  if (scalar?.toLowerCase().includes(query)) {
    return { fieldPath: path, preview: `${path}: ${summaryOf(value)}` }
  }
  if (value.kind === 'object') {
    if (value.value.actual_type.toLowerCase().includes(query)) {
      return { fieldPath: path, preview: `${path}: ${value.value.actual_type}` }
    }
    for (const [name, child] of Object.entries(value.value.fields)) {
      const childPath = `${path}.${name}`
      if (name.toLowerCase().includes(query)) {
        return { fieldPath: childPath, preview: `${childPath}: ${summaryOf(child)}` }
      }
      const match = valueMatch(child, query, childPath)
      if (match) return match
    }
  } else if (value.kind === 'array') {
    for (let index = 0; index < value.value.length; index += 1) {
      const match = valueMatch(value.value[index], query, `${path}[${index}]`)
      if (match) return match
    }
  } else if (value.kind === 'dict') {
    for (const [key, child] of value.value) {
      const keyText = key.kind === 'enum'
        ? key.value.variant ?? String(key.value.value)
        : String(key.value)
      const childPath = `${path}[${keyText}]`
      if (keyText.toLowerCase().includes(query)) {
        return { fieldPath: childPath, preview: `${childPath}: ${summaryOf(child)}` }
      }
      const match = valueMatch(child, query, childPath)
      if (match) return match
    }
  }
  return null
}

function scalarSearchText(value: FieldValue): string | null {
  switch (value.kind) {
    case 'option_none': return 'None'
    case 'option_some': return scalarSearchText(value.value)
    case 'result_ok': return scalarSearchText(value.value)
    case 'result_err': return scalarSearchText(value.value)
    case 'bool': return String(value.value)
    case 'int': return String(value.value)
    case 'float': return String(value.value)
    case 'string': return value.value
    case 'enum': return `${value.value.enum_name} ${value.value.variant ?? ''} ${value.value.value}`
    case 'ref': return value.value
    default: return null
  }
}
