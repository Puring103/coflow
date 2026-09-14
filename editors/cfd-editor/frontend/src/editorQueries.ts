import type { QueryClient } from '@tanstack/react-query'
import type { FileRecords } from './bindings/FileRecords'
import { editorQueryKeys } from './queryKeys'

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
      const records = await load()
      if (records.revision !== revision) {
        throw new Error(`文件数据版本不匹配：期望 ${revision}，收到 ${records.revision}`)
      }
      return records
    },
  })
}

export function removeSessionQueries(client: QueryClient, sessionId: number): void {
  // 所有编辑器 query key 的第二项都是 session id。
  client.removeQueries({ predicate: query => query.queryKey[1] === sessionId })
}
