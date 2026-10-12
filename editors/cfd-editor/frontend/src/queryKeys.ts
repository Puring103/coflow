export const editorQueryKeys = {
  fileRecords: (sessionId: number, revision: number, file: string) => (
    ['file-records', sessionId, revision, file] as const
  ),
  dimensionRecords: (sessionId: number, revision: number, file: string) => (
    ['dimension-records', sessionId, revision, file] as const
  ),
  graph: (sessionId: number, revision: number, file: string, depth: number, limit: number) => (
    ['graph', sessionId, revision, file, depth, limit] as const
  ),
  projectDiff: (sessionId?: number, revision?: number) => (
    ['project-diff', sessionId, revision] as const
  ),
}
