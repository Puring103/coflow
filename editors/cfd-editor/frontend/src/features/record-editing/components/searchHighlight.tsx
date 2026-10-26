import { type ReactNode } from 'react'

// 字段摘要与表格共用的搜索高亮。
export function highlightSearchText(text: string, query?: string): ReactNode {
  const needle = query?.trim()
  if (!needle) return text
  const lowerText = text.toLocaleLowerCase()
  const lowerNeedle = needle.toLocaleLowerCase()
  const parts: ReactNode[] = []
  let offset = 0
  let index = lowerText.indexOf(lowerNeedle, offset)
  while (index !== -1) {
    if (index > offset) parts.push(text.slice(offset, index))
    parts.push(<mark className="search-highlight" key={index}>{text.slice(index, index + needle.length)}</mark>)
    offset = index + needle.length
    index = lowerText.indexOf(lowerNeedle, offset)
  }
  if (offset < text.length) parts.push(text.slice(offset))
  return parts.length > 0 ? parts : text
}
