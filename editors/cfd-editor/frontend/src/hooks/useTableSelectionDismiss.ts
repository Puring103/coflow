import { useEffect, type RefObject } from 'react'

/** 表格与维度表共享选区退出规则；浮层属于原单元格的编辑上下文。 */
export function useTableSelectionDismiss(root: RefObject<HTMLDivElement | null>, kind: 'value' | 'record' | null, clear: () => void) {
  useEffect(() => {
    if (!kind) return
    const dismiss = (event: PointerEvent) => {
      const target = event.target
      if (!(target instanceof Element)) return
      if (kind === 'record' && target !== root.current) return
      if (target.closest('.inspector-modal-backdrop, .inspector-panel, .searchable-select-menu, .context-menu, .reference-panel-overlay')) return
      const cell = target.closest('[data-table-cell-key]')
      if (cell && root.current?.contains(cell)) return
      clear()
      const focused = document.activeElement
      if (focused instanceof HTMLElement && root.current?.contains(focused)) focused.blur()
    }
    document.addEventListener('pointerdown', dismiss, true)
    return () => document.removeEventListener('pointerdown', dismiss, true)
  }, [root, kind, clear])
}
