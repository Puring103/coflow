import { useEditorNavigation } from '../../workspace/context/editContext'

/** 使用查询身份定位诊断，避免依赖单元格 DOM 或本地化错误文本。 */
export function LookupErrorBadge({ lookupKey, message }: { lookupKey: string; message: string }) {
  const navigation = useEditorNavigation()
  return <button type="button" className="dc-load-error" title={message} aria-label={`查看加载错误：${message}`} onClick={event => {
    event.stopPropagation()
    navigation?.focusLookupError?.(lookupKey)
  }}>!</button>
}
