import type { QueryClient } from '@tanstack/react-query'
import type { FileRecords } from './bindings/FileRecords'
import { editorQueryKeys } from './queryKeys'

export function validateQueryRevision<T extends { revision: number }>(
  value: T,
  expectedRevision: number,
): T {
  if (value.revision !== expectedRevision) {
    throw new Error(`数据版本不匹配：期望 ${expectedRevision}，收到 ${value.revision}`)
  }
  return value
}

export function fetchFileRecords(
  client: QueryClient,
  sessionId: number,
  revision: number,
  filePath: string,
  load: () => Promise<FileRecords>,
): Promise<FileRecords> {
  return client.fetchQuery({
    queryKey: editorQueryKeys.fileRecords(sessionId, revision, filePath),
    queryFn: async () => {
      return validateQueryRevision(await load(), revision)
    },
  })
}

// 导航预取与源码视图共用请求，同一会话版本只读取一次；冲突检查仍直接读取磁盘。
export function fetchSourceText(
  client: QueryClient,
  sessionId: number,
  revision: number,
  filePath: string,
  load: () => Promise<string>,
): Promise<string> {
  return client.fetchQuery({
    queryKey: editorQueryKeys.sourceText(sessionId, revision, filePath),
    queryFn: load,
    staleTime: Number.POSITIVE_INFINITY,
  })
}

export function removeSessionQueries(client: QueryClient, sessionId: number): void {
  // 所有编辑器 query key 的第二项都是 session id。
  client.removeQueries({ predicate: query => query.queryKey[1] === sessionId })
}
