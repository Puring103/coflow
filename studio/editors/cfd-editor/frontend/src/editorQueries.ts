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
  const key = editorQueryKeys.fileRecords(sessionId, filePath)
  const cached = client.getQueryData<FileRecords>(key)
  if (cached && cached.revision !== revision) client.removeQueries({ queryKey: key, exact: true })
  return client.fetchQuery({
    queryKey: editorQueryKeys.fileRecords(sessionId, filePath),
    queryFn: async () => {
      return validateQueryRevision(await load(), revision)
    },
  })
}

export function removeSessionQueries(client: QueryClient, sessionId: number): void {
  // 所有编辑器 query key 的第二项都是 session id。
  client.removeQueries({ predicate: query => query.queryKey[1] === sessionId })
}
