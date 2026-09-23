import { describe, expect, it, vi } from 'vitest'
import type { RecordRow } from '../bindings/RecordRow'
import type { FileRecords } from '../bindings/FileRecords'
import { applyFileRecordsPatch } from './fileChanges'
import { ProjectGenerationController, publishMutationGeneration } from './editorState'
import { fromIpc, toIpc } from '../wire/ipc'
import { EditorProjectionReader } from '../hooks/useEditorProjections'
import { queryClient } from '../queryClient'
import { editorQueryKeys } from '../queryKeys'
import { MOCK_GRAPH } from '../mock'
import { graphCacheKey } from './appSupport'

const row = (key: string, index: number): RecordRow => ({
  coordinate: { actual_type: 'Item', key }, display_path: 'items.cfd',
  container_index: index, container_size: 2, field_diagnostics: [], diagnostic_severity: null, fields: [], field_index: {}, formatted_previews: {}, field_summaries: {},
})
const records = (rows: RecordRow[], revision = 1): FileRecords => ({
  revision, file_path: 'items.cfd', type_names: ['Item'], columns: [], records: rows,
  capabilities: { can_edit_field: true, can_edit_key: true, can_insert_record: true,
    can_delete_record: true, can_reorder_records: true, requires_full_refresh_after_write: false },
})

describe('file generation patches', () => {
  it('selects the latest graph and drops its references when queries are removed', () => {
    const generation = new ProjectGenerationController()
    generation.adopt({ session_id: 4324, revision: 2 })
    const reader = new EditorProjectionReader(generation)
    const graphKey = graphCacheKey('items.cfd', 2, 100)
    const oldKey = editorQueryKeys.graph(4324, 1, 'items.cfd', 2, 100)
    const newKey = editorQueryKeys.graph(4324, 2, 'items.cfd', 2, 100)
    queryClient.setQueryData(oldKey, { ...MOCK_GRAPH, revision: 1 })
    queryClient.setQueryData(newKey, { ...MOCK_GRAPH, revision: 2 })
    expect(reader.read().graphs[graphKey].revision).toBe(2)
    queryClient.removeQueries({ queryKey: newKey, exact: true })
    expect(reader.read().graphs[graphKey].revision).toBe(1)
    reader.reset()
    expect(reader.read().graphs).toEqual({})
  })
  it('only notifies projections for changed data in their own session and observes removal', () => {
    const generation = new ProjectGenerationController()
    generation.adopt({ session_id: 4322, revision: 1 })
    const reader = new EditorProjectionReader(generation)
    const listener = vi.fn()
    const unsubscribe = reader.subscribe(listener)
    const empty = reader.read()
    queryClient.setQueryData(['unrelated', 4322], 'value')
    queryClient.setQueryData(editorQueryKeys.fileRecords(4323, 'items.cfd'), records([]))
    expect(listener).not.toHaveBeenCalled()
    expect(reader.read()).toBe(empty)
    const key = editorQueryKeys.fileRecords(4322, 'items.cfd')
    queryClient.setQueryData(key, records([row('a', 0)]))
    const published = reader.read()
    expect(listener).toHaveBeenCalledTimes(1)
    expect(reader.read()).toBe(published)
    queryClient.invalidateQueries({ queryKey: key, exact: true })
    expect(listener).toHaveBeenCalledTimes(1)
    queryClient.removeQueries({ queryKey: key, exact: true })
    expect(reader.read().files).toEqual({})
    expect(listener).toHaveBeenCalledTimes(2)
    unsubscribe()
    queryClient.removeQueries({ predicate: query => query.queryKey[1] === 4322 || query.queryKey[1] === 4323 })
  })
  it('reuses a cached snapshot for a no-op mutation', async () => {
    const previous = records([row('a', 0)])
    const getFileRecords = vi.fn()
    const publishFileRecords = vi.fn()
    const result = await publishMutationGeneration({
      acceptRevision: () => true, isCurrent: () => true, getFileRecords,
      cachedFileRecords: () => previous, publishFileRecords,
    }, {
      sessionId: 1, revision: 1, diagnostics: [], affectedFiles: [], fallbackFile: 'items.cfd',
      changes: { base_revision: 1, revision: 1, files: [] },
    })
    expect(result.status).toBe('committed')
    expect(getFileRecords).not.toHaveBeenCalled()
    expect(publishFileRecords.mock.calls[0][0][0][1]).toBe(previous)
  })
  it('queries a complete target snapshot when the patch baseline is unavailable', async () => {
    const a = row('a', 0)
    const complete = records([a, row('b', 1)], 2)
    const getFileRecords = vi.fn(async () => complete)
    const publishFileRecords = vi.fn()
    await publishMutationGeneration({
      acceptRevision: () => true, isCurrent: () => true, getFileRecords, publishFileRecords,
    }, {
      sessionId: 1, revision: 2, diagnostics: [], affectedFiles: [], fallbackFile: 'items.cfd',
      changes: { base_revision: 1, revision: 2, files: [{ data: records([a], 2), order: complete.records.map(row => row.coordinate) }] },
    })
    expect(getFileRecords).toHaveBeenCalledExactlyOnceWith(1, 'items.cfd')
    expect(publishFileRecords.mock.calls[0][0][0][1]).toBe(complete)
  })
  it('does not allow an old snapshot request to overwrite a published projection', async () => {
    const generation = new ProjectGenerationController()
    generation.adopt({ session_id: 4321, revision: 1 })
    const reader = new EditorProjectionReader(generation)
    const key = editorQueryKeys.fileRecords(4321, 'items.cfd')
    let complete!: (value: FileRecords) => void
    const pending = queryClient.fetchQuery({
      queryKey: key, queryFn: () => new Promise<FileRecords>(resolve => { complete = resolve }),
    }).catch(() => undefined)
    const latest = records([row('new', 0)], 2)
    generation.acceptMutation(4321, 2)
    reader.setFiles({ 'items.cfd': latest })
    complete(records([row('old', 0)], 1))
    await pending
    expect(reader.read().files['items.cfd']).toBe(latest)
    reader.reset()
  })
  it('retains unchanged row identity and replaces only changed rows', () => {
    const a = row('a', 0)
    const b = row('b', 1)
    const changed = { ...a, diagnostic_severity: 'error' }
    const patch = { data: records([changed], 2), order: [a.coordinate, b.coordinate] }
    const result = applyFileRecordsPatch(records([a, b]), patch, 1)!
    expect(result.records[0]).toBe(changed)
    expect(result.records[1]).toBe(b)
    expect(result.revision).toBe(2)
  })
  it('uses the authoritative order for deletion and container positions', () => {
    const a = row('a', 0)
    const b = row('b', 1)
    const result = applyFileRecordsPatch(records([a, b]), { data: records([], 2), order: [b.coordinate] }, 1)!
    expect(result.records).toEqual([{ ...b, container_index: 0, container_size: 1 }])
    expect(applyFileRecordsPatch(records([a], 7), { data: records([], 2), order: [] }, 1)).toBeUndefined()
  })
  it('publishes a matching cached generation without fetching the full file', async () => {
    const a = row('a', 0)
    const previous = records([a, row('b', 1)])
    const getFileRecords = vi.fn()
    const publishFileRecords = vi.fn()
    await publishMutationGeneration({
      acceptRevision: () => true, isCurrent: () => true, getFileRecords,
      cachedFileRecords: () => previous, publishFileRecords,
    }, {
      sessionId: 1, revision: 2, diagnostics: [], affectedFiles: ['items.cfd'], fallbackFile: 'items.cfd',
      changes: { base_revision: 1, revision: 2, files: [{ data: records([a], 2), order: previous.records.map(row => row.coordinate) }] },
    })
    expect(getFileRecords).not.toHaveBeenCalled()
    expect(publishFileRecords).toHaveBeenCalledOnce()
  })
})

describe('IPC ownership', () => {
  it('decodes a fresh response without cloning its arrays or records', () => {
    const item = { kind: 'int', value: '4' }
    const input = { items: [item] }
    expect(fromIpc(input)).toBe(input)
    expect(input.items[0]).toBe(item)
    expect(item.value).toBe(4n)
  })
  it('encodes only BigInt branches without mutating caller state', () => {
    const unchanged = { label: 'same' }
    const input = { unchanged, field: { kind: 'int', value: 4n } }
    const output = toIpc(input) as typeof input
    expect(output.unchanged).toBe(unchanged)
    expect(output.field.value).toBe('4')
    expect(input.field.value).toBe(4n)
  })
})
