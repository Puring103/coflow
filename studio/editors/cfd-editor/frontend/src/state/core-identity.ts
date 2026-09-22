/** 世代身份唯一定义：会话 + 数据版本，编辑器与插件共享同一类型。 */
export interface GenerationIdentity {
  sessionId: number
  revision: number
}

export function sameIdentity(
  a: GenerationIdentity | null | undefined,
  b: GenerationIdentity | null | undefined,
): boolean {
  if (!a || !b) return false
  return a.sessionId === b.sessionId && a.revision === b.revision
}
