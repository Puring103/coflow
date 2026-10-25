import { createContext } from 'react'
import type { FieldValue } from '../wire'

/** Ctrl+Enter 的批量填充由拥有选区的视图负责，输入组件只提交当前草稿。 */
export const DataFillContext = createContext<((value: FieldValue) => Promise<void>) | null>(null)
