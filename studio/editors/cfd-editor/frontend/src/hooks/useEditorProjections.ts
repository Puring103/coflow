import { useMemo, useSyncExternalStore, type SetStateAction } from 'react'
import type { Query, QueryCache } from '@tanstack/react-query'
import type { FileRecords } from '../bindings/FileRecords'
import type { GraphData } from '../bindings/GraphData'
import type { ProjectGenerationController } from '../state/editorState'
import { queryClient } from '../queryClient'
import { editorQueryKeys } from '../queryKeys'
import { graphCacheKey } from '../state/appSupport'

type FileProjection = Record<string, FileRecords>
type GraphProjection = Record<string, GraphData>
function resolveState<T>(action: SetStateAction<T>, current: T): T {
  return typeof action === 'function' ? (action as (previous: T) => T)(current) : action
}
type ProjectionSnapshot = { files: FileProjection; graphs: GraphProjection }
const EMPTY_PROJECTION: ProjectionSnapshot = { files: {}, graphs: {} }

/** 索引只保存 QueryCache 数据的引用；事件只更新对应文件或图，不在读取期间扫描缓存。 */
class EditorProjectionIndex {
  private readonly sessions = new Map<number, ProjectionSnapshot>()
  private readonly graphVersions = new Map<string, Map<string, GraphData>>()
  private readonly listeners = new Set<(sessionId: number) => void>()

  constructor(cache: QueryCache) {
    for (const query of cache.getAll()) this.update(query, false)
    cache.subscribe(event => {
      if (event.type === 'added' || event.type === 'updated' || event.type === 'removed') {
        this.update(event.query, event.type === 'removed')
      }
    })
  }

  read(sessionId: number | null): ProjectionSnapshot {
    return sessionId === null ? EMPTY_PROJECTION : this.sessions.get(sessionId) ?? EMPTY_PROJECTION
  }

  subscribe(listener: (sessionId: number) => void): () => void {
    this.listeners.add(listener)
    return () => { this.listeners.delete(listener) }
  }

  private update(query: Query, removed: boolean): void {
    const key = query.queryKey
    const sessionId = key[1]
    if (typeof sessionId !== 'number' || (key[0] !== 'file-records' && key[0] !== 'graph')) return
    const previous = this.read(sessionId)
    let next = previous
    if (key[0] === 'file-records') {
      const file = key[2] as string
      const data = removed ? undefined : query.state.data as FileRecords | undefined
      if (previous.files[file] === data) return
      const files = { ...previous.files }
      if (data) files[file] = data
      else delete files[file]
      next = { ...previous, files }
    } else {
      const graphKey = graphCacheKey(key[3] as string, key[4] as number, key[5] as number)
      const bucketKey = JSON.stringify([sessionId, graphKey])
      const versions = this.graphVersions.get(bucketKey) ?? new Map<string, GraphData>()
      const data = removed ? undefined : query.state.data as GraphData | undefined
      if (versions.get(query.queryHash) === data) return
      if (data) versions.set(query.queryHash, data)
      else versions.delete(query.queryHash)
      if (versions.size) this.graphVersions.set(bucketKey, versions)
      else this.graphVersions.delete(bucketKey)
      let latest: GraphData | undefined
      for (const graph of versions.values()) {
        if (!latest || graph.revision > latest.revision) latest = graph
      }
      if (previous.graphs[graphKey] === latest) return
      const graphs = { ...previous.graphs }
      if (latest) graphs[graphKey] = latest
      else delete graphs[graphKey]
      next = { ...previous, graphs }
    }
    if (Object.keys(next.files).length || Object.keys(next.graphs).length) this.sessions.set(sessionId, next)
    else this.sessions.delete(sessionId)
    for (const listener of this.listeners) listener(sessionId)
  }
}

// 生命周期与应用级 QueryClient 一致，避免每个组件单独订阅和建立相同索引。
const projectionIndex = new EditorProjectionIndex(queryClient.getQueryCache())

export class EditorProjectionReader {
  readonly filesRef: { current: FileProjection }
  readonly graphsRef: { current: GraphProjection }
  constructor(private generation: ProjectGenerationController) {
    const owner = this
    this.filesRef = { get current() { return owner.read().files } }
    this.graphsRef = { get current() { return owner.read().graphs } }
  }
  subscribe = (listener: () => void) => projectionIndex.subscribe(sessionId => {
    if (sessionId === this.generation.currentSession()) listener()
  })
  read = () => projectionIndex.read(this.generation.currentSession())
  setFiles = (action: SetStateAction<FileProjection>) => {
    const identity = this.generation.currentIdentity()
    if (!identity) return
    const next = resolveState(action, this.read().files)
    for (const query of queryClient.getQueryCache().findAll({ queryKey: ['file-records', identity.sessionId] })) {
      if (!((query.queryKey[2] as string) in next)) queryClient.removeQueries({ queryKey: query.queryKey, exact: true })
    }
    for (const [file, data] of Object.entries(next)) {
      const key = editorQueryKeys.fileRecords(identity.sessionId, file)
      if (queryClient.getQueryData(key) !== data) {
        // 取消旧请求但不回滚缓存，防止迟到的完整快照覆盖已发布的增量或乐观编辑。
        void queryClient.cancelQueries({ queryKey: key, exact: true }, { revert: false })
        queryClient.setQueryData(key, data)
      }
    }
  }
  setGraphs = (action: SetStateAction<GraphProjection>) => {
    const identity = this.generation.currentIdentity()
    if (!identity) return
    const next = resolveState(action, this.read().graphs)
    for (const [cacheKey, data] of Object.entries(next)) {
      const parts = cacheKey.split('::')
      const limit = Number(parts.pop())
      const depth = Number(parts.pop())
      const file = parts.join('::')
      const key = editorQueryKeys.graph(identity.sessionId, data.revision, file, depth, limit)
      if (queryClient.getQueryData(key) !== data) {
        // 取消旧请求但不回滚缓存，防止迟到的完整快照覆盖已发布的增量或乐观编辑。
        void queryClient.cancelQueries({ queryKey: key, exact: true }, { revert: false })
        queryClient.setQueryData(key, data)
      }
    }
    // 保留最新图代际；旧结果不在组件外再持有完整字段树。
    for (const query of queryClient.getQueryCache().findAll({ queryKey: ['graph', identity.sessionId] })) {
      const key = query.queryKey
      const data = next[graphCacheKey(key[3] as string, key[4] as number, key[5] as number)]
      if (!data || (key[2] as number) < data.revision) queryClient.removeQueries({ queryKey: key, exact: true })
    }
  }
  reset = () => {
    const id = this.generation.currentSession()
    queryClient.removeQueries({ predicate: query => query.queryKey[1] === id })
    this.read()
  }
}

export function useEditorProjections(generation: ProjectGenerationController) {
  const reader = useMemo(() => new EditorProjectionReader(generation), [generation])
  const snapshot = useSyncExternalStore(reader.subscribe, reader.read, reader.read)
  return { ...snapshot, filesRef: reader.filesRef, graphsRef: reader.graphsRef,
    setFiles: reader.setFiles, setGraphs: reader.setGraphs, reset: reader.reset }
}
