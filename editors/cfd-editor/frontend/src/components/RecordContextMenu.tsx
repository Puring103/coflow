import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { sameCoordinate } from '../wire'
import { Icon } from './Icon'
import { recordGroupColorStyle } from './RecordGroupHeader'
import { fitViewportPosition } from '../utils/floatingPosition'

// 记录右键菜单在表格、记录和图视图中共用；各项由调用方按能力决定是否出现。
export interface RecordContextMenuRequest {
  anchorX: number
  anchorY: number
  filePath: string
  coordinates: readonly RecordCoordinate[]
  /** 单条上下文用于展示；多选时传 null。 */
  primaryKey?: string | null
}

interface Props {
  request: RecordContextMenuRequest
  groups?: readonly EditorRecordGroup[]
  showOpenRecord?: boolean
  canRename?: boolean
  canInsertBelow?: boolean
  canDelete?: boolean
  canAddToGroup?: boolean
  canCreateGroup?: boolean
  onOpenRecord?: () => void
  onRename?: () => void
  onInsertBelow?: () => void
  onDelete?: () => void
  onCreateGroup?: (coordinates: readonly RecordCoordinate[]) => void
  onAddToGroup?: (coordinates: readonly RecordCoordinate[], groupId: string) => void
  onClose: () => void
}

export function RecordContextMenu({
  request,
  groups,
  showOpenRecord,
  canRename,
  canInsertBelow,
  canDelete,
  canAddToGroup,
  canCreateGroup,
  onOpenRecord,
  onRename,
  onInsertBelow,
  onDelete,
  onCreateGroup,
  onAddToGroup,
  onClose,
}: Props) {
  const menuRef = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ x: request.anchorX, y: request.anchorY })
  const [showGroupTargets, setShowGroupTargets] = useState(false)
  const multiple = request.coordinates.length > 1

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose() }
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Element && event.target.closest('.record-context-menu')) return
      onClose()
    }
    window.addEventListener('keydown', onKey)
    window.addEventListener('pointerdown', onPointerDown, true)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('pointerdown', onPointerDown, true)
    }
  }, [onClose])

  // 菜单可能超出视口，挂载后按真实尺寸回填位置；展开分组子项后重新计算。
  useLayoutEffect(() => {
    const fit = () => {
      const menu = menuRef.current
      if (!menu) return
      const rect = menu.getBoundingClientRect()
      const next = fitViewportPosition(
        { x: request.anchorX, y: request.anchorY },
        { width: rect.width, height: rect.height },
        { width: window.innerWidth, height: window.innerHeight },
      )
      setPosition(current => current.x === next.x && current.y === next.y ? current : next)
    }
    fit()
    window.addEventListener('resize', fit)
    return () => window.removeEventListener('resize', fit)
  }, [request.anchorX, request.anchorY, showGroupTargets, groups?.length, canAddToGroup])

  return createPortal(
    <div
      ref={menuRef}
      className="context-menu table-context-menu record-context-menu"
      style={{ left: position.x, top: position.y }}
      onClick={event => event.stopPropagation()}
      role="menu"
    >
      {showOpenRecord && onOpenRecord && (
        <div className="ctx-item" role="menuitem" onClick={() => { onOpenRecord(); onClose() }}>
          <Icon name="record" size={13} aria-hidden />
          跳转到记录视图
        </div>
      )}
      {canAddToGroup && (<>
        <div className="ctx-sep" />
        <button
          type="button"
          className="ctx-item"
          role="menuitem"
          aria-expanded={showGroupTargets}
          onClick={() => setShowGroupTargets(current => !current)}
        >
          <Icon name="plus" size={13} aria-hidden />
          添加到分组
          <span className="ctx-item-tail">
            {multiple && <span>{request.coordinates.length} 条</span>}
            <Icon name={showGroupTargets ? 'chevron-down' : 'chevron-right'} size={12} aria-hidden />
          </span>
        </button>
        {showGroupTargets && (
          <div className="ctx-group-targets" role="group" aria-label="选择分组">
            {multiple && canCreateGroup && onCreateGroup && (
              <button
                type="button"
                className="ctx-item ctx-group-target"
                role="menuitem"
                onClick={() => { onCreateGroup(request.coordinates); onClose() }}
              >
                <Icon name="plus" size={13} aria-hidden />
                新建分组
              </button>
            )}
            {onAddToGroup && groups?.map(group => {
              const alreadyInGroup = request.coordinates.every(coordinate => (
                group.records.some(member => sameCoordinate(member, coordinate))
              ))
              return (
                <button
                  key={group.id}
                  type="button"
                  className="ctx-item ctx-group-target"
                  role="menuitem"
                  disabled={alreadyInGroup}
                  title={alreadyInGroup ? '所选记录已在此分组中' : undefined}
                  onClick={() => { onAddToGroup(request.coordinates, group.id); onClose() }}
                >
                  <span
                    className={`ctx-group-color${group.color ? ' has-color' : ''}`}
                    style={recordGroupColorStyle(group.color)}
                    aria-hidden
                  />
                  <span className="ctx-group-name">{group.name}</span>
                  <span className="ctx-shortcut">{group.records.length}</span>
                </button>
              )
            })}
          </div>
        )}
      </>)}
      {!multiple && canRename && onRename && (
        <div className="ctx-item" role="menuitem" onClick={() => { onRename(); onClose() }}>
          <Icon name="edit" size={13} aria-hidden />
          重命名 Key
        </div>
      )}
      {!multiple && canInsertBelow && onInsertBelow && (
        <div className="ctx-item" role="menuitem" onClick={() => { onInsertBelow(); onClose() }}>
          <Icon name="plus" size={13} aria-hidden />
          在下方插入记录
        </div>
      )}
      {canDelete && onDelete && (
        <div className="ctx-item ctx-danger" role="menuitem" onClick={() => { onDelete(); onClose() }}>
          <Icon name="close" size={13} aria-hidden />
          {multiple ? `删除 ${request.coordinates.length} 条记录` : '删除记录'}
        </div>
      )}
    </div>,
    document.body,
  )
}
