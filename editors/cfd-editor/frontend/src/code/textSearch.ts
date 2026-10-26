/** 字面文本搜索保留原文偏移；正则转义避免标点被解释为搜索语法。 */
export function textSearchMatches(source: string, query: string): { from: number; to: number }[] {
  if (!query) return []
  const expression = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'giu')
  return Array.from(source.matchAll(expression), match => ({ from: match.index, to: match.index + match[0].length }))
}
