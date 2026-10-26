import { useEffect, useRef } from 'react'
import type { RefObject, PointerEvent as ReactPointerEvent, MouseEvent as ReactMouseEvent } from 'react'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import { coordinateId } from '../wire'

interface Options {
  rootRef: RefObject<HTMLElement | null>
  records: readonly RecordRow[]
  selectedCoordinates?: readonly RecordCoordinate[]
  onSelectDragSource?: (source: RecordCoordinate) => void
  onDropRecordOntoRecord?: (sources: readonly RecordCoordinate[], target: RecordCoordinate) => void
  onDropRecordAfterRecord?: (sources: readonly RecordCoordinate[], target: RecordCoordinate) => void
  onDropRecordIntoGroup?: (sources: readonly RecordCoordinate[], groupId: string) => void
  onDropRecordIntoUngrouped?: (sources: readonly RecordCoordinate[]) => void
}

const DRAG_THRESHOLD = 5

export function useRecordPointerDrag(options: Options) {
  const optionsRef = useRef(options)
  const cleanupRef = useRef<(() => void) | null>(null)
  const suppressClickRef = useRef(false)
  optionsRef.current = options

  useEffect(() => () => cleanupRef.current?.(), [])

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || cleanupRef.current) return
    const root = optionsRef.current.rootRef.current
    const target = event.target as HTMLElement
    const sourceElement = target.closest<HTMLElement>('[data-record-draggable="true"]')
    if (!root || !sourceElement || !root.contains(sourceElement)) return
    const interactive = target.closest('button, input, select, textarea, a, [contenteditable="true"]')
    if (interactive && !target.closest('.record-drag-handle')) return

    const sourceId = sourceElement.dataset.coordinateId
    if (!sourceId) return
    const source = optionsRef.current.records.find(record => coordinateId(record.coordinate) === sourceId)
    if (!source) return
    const selected = optionsRef.current.selectedCoordinates ?? []
    const sources = selected.some(item => coordinateId(item) === sourceId)
      ? selected
      : [source.coordinate]
    const selectSourceOnDrag = sources.length === 1 && sources[0] === source.coordinate
    const startX = event.clientX
    const startY = event.clientY
    const sourceRow = sourceElement.closest<HTMLTableRowElement>('tr')
    const rowBounds = sourceRow?.getBoundingClientRect()
    const viewportBounds = root.getBoundingClientRect()
    const previewLeft = rowBounds ? Math.max(viewportBounds.left, rowBounds.left) : startX
    const previewWidth = rowBounds ? Math.min(viewportBounds.right, rowBounds.right) - previewLeft : 0
    let dragging = false
    let dropTarget: HTMLElement | null = null
    let insertTarget: HTMLElement | null = null
    let preview: HTMLDivElement | null = null

    const clearDropTarget = () => {
      dropTarget?.classList.remove('drag-over')
      dropTarget = null
      insertTarget?.classList.remove('record-insert-target')
      insertTarget = null
    }
    const cleanup = () => {
      clearDropTarget()
      sourceElement.classList.remove('record-dragging')
      sourceRow?.classList.remove('record-row-dragging')
      document.body.classList.remove('record-drag-active')
      preview?.remove()
      preview = null
      document.removeEventListener('pointermove', onPointerMove)
      document.removeEventListener('pointerup', onPointerUp)
      document.removeEventListener('pointercancel', onPointerCancel)
      cleanupRef.current = null
    }
    const beginDrag = () => {
      dragging = true
      suppressClickRef.current = true
      if (selectSourceOnDrag) optionsRef.current.onSelectDragSource?.(source.coordinate)
      sourceElement.classList.add('record-dragging')
      document.body.classList.add('record-drag-active')
      preview = document.createElement('div')
      preview.className = 'record-drag-preview'
      const label = sourceElement.dataset.recordLabel || sourceId
      if (sourceRow && rowBounds) {
        // 只复制当前可见行的展示内容；副本不可交互，真实记录仍由原有落点逻辑移动。
        preview.classList.add('app', 'record-row-preview')
        preview.inert = true
        preview.setAttribute('aria-hidden', 'true')
        preview.style.width = `${previewWidth}px`
        preview.style.height = `${rowBounds.height}px`
        const table = document.createElement('table')
        table.className = 'data-table'
        const body = document.createElement('tbody')
        const clone = sourceRow.cloneNode(true) as HTMLTableRowElement
        clone.classList.remove('record-row-dragging')
        clone.querySelectorAll('[id]').forEach(node => node.removeAttribute('id'))
        for (let index = 0; index < sourceRow.cells.length; index++) {
          const rect = sourceRow.cells[index].getBoundingClientRect()
          const cell = clone.cells[index]
          Object.assign(cell.style, { position: 'absolute', left: `${rect.left - previewLeft}px`, top: '0', width: `${rect.width}px`, height: `${rect.height}px`, boxSizing: 'border-box' })
          cell.removeAttribute('data-cell-motion')
        }
        body.append(clone); table.append(body); preview.append(table)
        sourceRow.classList.add('record-row-dragging')
        if (sources.length > 1) {
          const count = document.createElement('span')
          count.className = 'record-drag-count'
          count.textContent = `${sources.length} 条记录`
          preview.append(count)
        }
      } else preview.textContent = sources.length > 1 ? `${label} 等 ${sources.length} 条记录` : label
      document.body.appendChild(preview)
    }
    const updateDropTarget = (clientX: number, clientY: number) => {
      const hit = document.elementFromPoint(clientX, clientY)
        ?.closest<HTMLElement>('[data-record-drop-kind]') ?? null
      const next = hit && root.contains(hit) ? hit : null
      const canInsertAfter = next?.dataset.recordDropKind === 'record'
        && !!optionsRef.current.onDropRecordAfterRecord
      const rect = next?.getBoundingClientRect()
      const edgeThreshold = rect ? Math.min(8, rect.height / 4) : 0
      const nearBottom = !!rect && clientY >= rect.bottom - edgeThreshold
      const nearTop = !!rect && clientY <= rect.top + edgeThreshold
      const sameRecord = next?.dataset.recordDropKind === 'record'
        && sources.some(item => coordinateId(item) === next.dataset.coordinateId)
      const previous = nearTop
        ? next?.previousElementSibling?.matches('[data-record-drop-kind="record"]')
          ? next.previousElementSibling as HTMLElement
          : null
        : null
      const insertion = canInsertAfter && !sameRecord
        ? (nearBottom ? next : previous)
        : null
      if (insertion) {
        if (insertTarget === insertion) return
        clearDropTarget()
        insertTarget = insertion
        insertTarget.classList.add('record-insert-target')
        return
      }
      const valid = sameRecord ? null : next
      if (valid === dropTarget) return
      clearDropTarget()
      dropTarget = valid
      dropTarget?.classList.add('drag-over')
    }
    const onPointerMove = (moveEvent: PointerEvent) => {
      if (!dragging) {
        const distance = Math.hypot(moveEvent.clientX - startX, moveEvent.clientY - startY)
        if (distance < DRAG_THRESHOLD) return
        beginDrag()
      }
      moveEvent.preventDefault()
      if (preview) {
        preview.style.left = `${sourceRow ? previewLeft + moveEvent.clientX - startX : moveEvent.clientX + 12}px`
        preview.style.top = `${rowBounds ? rowBounds.top + moveEvent.clientY - startY : moveEvent.clientY + 12}px`
      }
      updateDropTarget(moveEvent.clientX, moveEvent.clientY)
    }
    const finishDrop = () => {
      if (!dragging) return
      const current = optionsRef.current
      if (insertTarget) {
        const targetId = insertTarget.dataset.coordinateId
        const targetRecord = current.records.find(record => coordinateId(record.coordinate) === targetId)
        if (targetRecord) current.onDropRecordAfterRecord?.(sources, targetRecord.coordinate)
        return
      }
      if (!dropTarget) return
      const kind = dropTarget.dataset.recordDropKind
      if (kind === 'record') {
        const targetId = dropTarget.dataset.coordinateId
        const targetRecord = current.records.find(record => coordinateId(record.coordinate) === targetId)
        if (targetRecord) current.onDropRecordOntoRecord?.(sources, targetRecord.coordinate)
      } else if (kind === 'group') {
        const groupId = dropTarget.dataset.recordGroupId
        if (groupId) current.onDropRecordIntoGroup?.(sources, groupId)
      } else if (kind === 'ungrouped') {
        current.onDropRecordIntoUngrouped?.(sources)
      }
    }
    const onPointerUp = () => {
      finishDrop()
      cleanup()
      window.setTimeout(() => { suppressClickRef.current = false }, 0)
    }
    const onPointerCancel = () => {
      cleanup()
      suppressClickRef.current = false
    }

    document.addEventListener('pointermove', onPointerMove, { passive: false })
    document.addEventListener('pointerup', onPointerUp)
    document.addEventListener('pointercancel', onPointerCancel)
    cleanupRef.current = cleanup
  }

  const onClickCapture = (event: ReactMouseEvent<HTMLElement>) => {
    if (!suppressClickRef.current) return
    event.preventDefault()
    event.stopPropagation()
  }

  return { onPointerDown, onClickCapture }
}
