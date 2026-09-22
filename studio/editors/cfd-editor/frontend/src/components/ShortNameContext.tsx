import { createContext, useContext, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { EditorLookupContext } from '../utils/editContext'
import { Icon } from './Icon'

export const ShortNameContext = createContext<{
  fields: { [type: string]: string | undefined }
  setField?: (type: string, field: string | null) => void
}>({ fields: {} })

export function ShortNameMenuItem({ actualType, field, onClose }: {
  actualType: string
  field: string | undefined
  onClose: () => void
}) {
  const settings = useContext(ShortNameContext)
  if (!field || !settings.setField) return null
  const selected = settings.fields[actualType] === field
  return <button type="button" className="ctx-item" role="menuitemcheckbox" aria-checked={selected}
    onClick={() => { settings.setField!(actualType, selected ? null : field); onClose() }}>
    <Icon name={selected ? 'check' : 'edit'} size={13} aria-hidden />
    {selected ? '取消缩略名' : '设置为缩略名'}
  </button>
}

export function ShortNameColumnMenu({ actualType, field, x, y, onClose }: {
  actualType: string; field: string; x: number; y: number; onClose: () => void
}) {
  const menu = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const dismiss = (event: PointerEvent) => { if (!menu.current?.contains(event.target as Node)) onClose() }
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose() }
    window.addEventListener('pointerdown', dismiss)
    window.addEventListener('keydown', escape)
    window.addEventListener('blur', onClose)
    window.addEventListener('resize', onClose)
    return () => {
      window.removeEventListener('pointerdown', dismiss)
      window.removeEventListener('keydown', escape)
      window.removeEventListener('blur', onClose)
      window.removeEventListener('resize', onClose)
    }
  }, [onClose])
  return createPortal(<div ref={menu} className="context-menu" role="menu"
    style={{ left: Math.max(0, Math.min(x, window.innerWidth - 200)), top: Math.max(0, Math.min(y, window.innerHeight - 48)) }}>
    <ShortNameMenuItem actualType={actualType} field={field} onClose={onClose} />
  </div>, document.body)
}

/** 引用与折叠图节点按目标类型共享查询缓存，值或设置变化后随 lookup generation 刷新。 */
export function useReferenceShortName(targetType: string | undefined, key: string): string | undefined {
  const lookups = useContext(EditorLookupContext)
  const [resolved, setResolved] = useState<{ lookups: typeof lookups; type: string; key: string; name?: string } | null>(null)
  useEffect(() => {
    if (!lookups || !targetType) return
    let active = true
    void lookups.loadRefTargets(targetType).then(result => {
      if (active && result.ok) setResolved({ lookups, type: targetType, key,
        name: result.value.find(target => target.coordinate.key === key)?.short_name ?? undefined })
    })
    return () => { active = false }
  }, [lookups, targetType, key])
  // 同一会话刷新期间保留旧名称，避免每次字段保存都先回退为 key 再闪回名称。
  if (resolved?.lookups?.sessionId === lookups?.sessionId && resolved?.type === targetType && resolved?.key === key) return resolved.name
  return targetType ? lookups?.cachedRefTargets(targetType)?.find(target => target.coordinate.key === key)?.short_name ?? undefined : undefined
}
