import { useEffect, useRef } from 'react'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { FieldPathSegment } from '../wire'
import type { EditEntry, MutationHistorySnapshot } from '../state/editorState'
import { emphasize, type MotionOrigin } from '../utils/motion'

export function motionRecordKey(file: string, coordinate: RecordCoordinate): string {
  return JSON.stringify([file, coordinate.actual_type, coordinate.id])
}

/** 只读取已经提交的历史变化；乐观快照、保存失败和普通刷新不产生成功强调。 */
export function useMutationFeedback(snapshot: MutationHistorySnapshot, origin: MotionOrigin | null, modalOpen: boolean) {
  const previous = useRef(snapshot)
  const cleanups = useRef<(() => void)[]>([])
  const pendingFrame = useRef(0)
  useEffect(() => () => { cancelAnimationFrame(pendingFrame.current); cleanups.current.forEach(cleanup => cleanup()) }, [])
  useEffect(() => {
    const before = previous.current
    previous.current = snapshot
    const last = snapshot.undo[snapshot.undo.length - 1]
    const undone = snapshot.redo[snapshot.redo.length - 1]
    const entry = undone && snapshot.redo.length > before.redo.length ? undone
      : last && (snapshot.undo.length > before.undo.length || last.revision !== before.undo[before.undo.length - 1]?.revision) ? last : null
    if (!entry) {
      if (!snapshot.undo.length && !snapshot.redo.length) {
        cancelAnimationFrame(pendingFrame.current)
        cleanups.current.forEach(cleanup => cleanup())
        cleanups.current = []
      }
      return
    }
    cancelAnimationFrame(pendingFrame.current)
    pendingFrame.current = requestAnimationFrame(() => {
      cleanups.current.forEach(cleanup => cleanup())
      cleanups.current = []
      // 只索引已挂载的行，批量编辑不会展开屏外行或构造整张表的单元格集合。
      const visible = new Map<string, HTMLElement[]>()
      document.querySelectorAll<HTMLElement>('[data-motion-record]').forEach(element => {
        const key = element.dataset.motionRecord!
        const items = visible.get(key) ?? []
        items.push(element)
        visible.set(key, items)
      })
      const targets = new Set<HTMLElement>()
      const add = (file: string, coordinate: RecordCoordinate, path?: FieldPathSegment[]) => {
        for (const row of visible.get(motionRecordKey(file, coordinate)) ?? []) {
          const field = path?.[0]?.kind === 'field' ? path[0].value : undefined
          const exact = path ? row.querySelector<HTMLElement>(`[data-field-path-wire="${CSS.escape(JSON.stringify(path))}"]`) : null
          const cell = field ? row.querySelector<HTMLElement>(`[data-table-value-cell][data-field="${CSS.escape(field)}"]`) : null
          const target = exact ?? cell ?? row
          const source = origin?.resolve()
          if (modalOpen && source && (target === source || target.contains(source) || source.contains(target))) origin!.changed = true
          if (!target.closest('[inert]') && !target.contains(document.activeElement)) targets.add(target)
        }
      }
      const visit = (change: EditEntry) => {
        switch (change.kind) {
          case 'batch': change.entries.forEach(visit); break
          case 'batch-field': change.edits.forEach(edit => add(edit.filePath, edit.coordinate)); break
          case 'field': add(change.filePath, change.coordinate, change.fieldPath); break
          case 'insert': case 'delete': case 'move-record': add(change.filePath, change.coordinate); break
          case 'swap-records': add(change.filePath, change.first); add(change.filePath, change.second); break
          case 'transfer-record': add(change.filePath, change.coordinate); add(change.destinationFile, change.coordinate); break
          // 维度与图布局保持自己的选择提示，不把坐标变化表达成字段写入。
          case 'dimension': case 'graph-layout': break
        }
      }
      visit(entry)
      for (const target of targets) {
        const rect = target.getBoundingClientRect()
        if (rect.bottom > 0 && rect.top < innerHeight && rect.right > 0 && rect.left < innerWidth) cleanups.current.push(emphasize(target))
      }
    })
  }, [snapshot, origin, modalOpen])
}
