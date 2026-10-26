import { QueryClient, QueryObserver } from '@tanstack/react-query'
import { describe, expect, it, vi } from 'vitest'
import type { FileRecords } from '../bindings/FileRecords'
import { pruneObsoleteQueries, fetchFileRecords, fetchSourceText, removeSessionQueries, validateQueryRevision } from './editorQueries'
import { editorQueryKeys } from './queryKeys'

const records = (revision: number): FileRecords => ({
  revision, file_path: 'data.cfd', type_names: [], columns: [], records: [],
  capabilities: {
    can_edit_field: true,
    can_edit_id: true,
    can_insert_record: true,
    can_delete_record: true,
    can_reorder_records: true,
    requires_full_refresh_after_write: false,
  },
})

describe('editor queries', () => {
  it('deduplicates concurrent file requests for one revision', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const load = vi.fn(async () => records(3))
    const first = fetchFileRecords(client, 7, 3, 'data.cfd', load)
    const second = fetchFileRecords(client, 7, 3, 'data.cfd', load)
    await expect(Promise.all([first, second])).resolves.toEqual([records(3), records(3)])
    expect(load).toHaveBeenCalledTimes(1)
  })

  it('shares source prefetch with first mount and reloads after a project revision', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const load = vi.fn(async () => 'table Item {}')
    const prefetch = fetchSourceText(client, 7, 3, 'a.cft', load)
    const mounted = fetchSourceText(client, 7, 3, 'a.cft', load)
    await expect(Promise.all([prefetch, mounted])).resolves.toEqual(['table Item {}', 'table Item {}'])
    await fetchSourceText(client, 7, 3, 'a.cft', load)
    expect(load).toHaveBeenCalledTimes(1)
    await fetchSourceText(client, 7, 3, 'b.cft', load)
    await fetchSourceText(client, 7, 4, 'a.cft', load)
    await fetchSourceText(client, 8, 3, 'a.cft', load)
    expect(load).toHaveBeenCalledTimes(4)
  })

  it('removes only queries owned by the selected session', () => {
    const client = new QueryClient()
    client.setQueryData(editorQueryKeys.fileRecords(7, 1, 'a.cfd'), records(1))
    client.setQueryData(editorQueryKeys.fileRecords(8, 7, 'b.cfd'), records(7))
    removeSessionQueries(client, 7)
    expect(client.getQueryData(editorQueryKeys.fileRecords(7, 1, 'a.cfd'))).toBeUndefined()
    expect(client.getQueryData(editorQueryKeys.fileRecords(8, 7, 'b.cfd'))).toBeDefined()
  })

  it('does not cache a response from another revision', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    await expect(fetchFileRecords(client, 7, 3, 'data.cfd', async () => records(4)))
      .rejects.toThrow('期望 3，收到 4')
    expect(client.getQueryData(editorQueryKeys.fileRecords(7, 3, 'data.cfd'))).toBeUndefined()
  })

  it('uses one revision validator for every query response', () => {
    expect(validateQueryRevision(records(3), 3)).toEqual(records(3))
    expect(() => validateQueryRevision(records(4), 3)).toThrow('期望 3，收到 4')
  })
})

it('releases obsolete snapshots while preserving active, current and other-session data', () => {
  const client = new QueryClient()
  for (let revision = 1; revision <= 100; revision++) client.setQueryData(editorQueryKeys.fileRecords(7, revision, 'a.cfd'), records(revision))
  const activeKey = editorQueryKeys.fileRecords(7, 1, 'a.cfd')
  const observer = new QueryObserver(client, { queryKey: activeKey, staleTime: Infinity })
  const unsubscribe = observer.subscribe(() => {})
  client.setQueryData(editorQueryKeys.fileRecords(8, 1, 'a.cfd'), records(1))
  pruneObsoleteQueries(client, 7, 100)
  expect(client.getQueryCache().getAll()).toHaveLength(3)
  expect(client.getQueryData(activeKey)).toEqual(records(1))
  unsubscribe()
  pruneObsoleteQueries(client, 7, 100)
  expect(client.getQueryCache().getAll()).toHaveLength(2)
  client.clear()
})
