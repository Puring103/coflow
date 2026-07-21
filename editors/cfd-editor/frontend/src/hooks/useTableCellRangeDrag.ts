import { useEffect, useRef } from 'react'
import type { PointerEvent as ReactPointerEvent, RefObject } from 'react'

interface Options {
  rootRef: RefObject<HTMLElement | null>
  onSelectCell: (coordinateId: string, field: string, mode: 'replace' | 'range') => void
}

const DRAG_THRESHOLD = 4
const EDGE = 28

export function useTableCellRangeDrag(options: Options) {
  const optionsRef = useRef(options)
  const cleanupRef = useRef<(() => void) | null>(null)
  optionsRef.current = options
  useEffect(() => () => cleanupRef.current?.(), [])

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || cleanupRef.current) return
    const root = optionsRef.current.rootRef.current
    const target = event.target as HTMLElement
    const cell = target.closest<HTMLElement>('[data-table-value-cell="true"]')
    if (!root || !cell || !root.contains(cell)) return
    const coordinateId = cell.dataset.coordinateId
    const field = cell.dataset.field
    if (!coordinateId || !field) return
    const native = target.closest('input, select, textarea, button, a, [contenteditable="true"]')
    optionsRef.current.onSelectCell(coordinateId, field, event.shiftKey ? 'range' : 'replace')
    if (native) return

    const startX = event.clientX
    const startY = event.clientY
    let dragging = false
    let clientX = startX
    let clientY = startY
    let frame = 0

    const selectAtPoint = () => {
      const rect = root.getBoundingClientRect()
      const hit = document.elementFromPoint(
        Math.max(rect.left + 1, Math.min(rect.right - 1, clientX)),
        Math.max(rect.top + 1, Math.min(rect.bottom - 1, clientY)),
      )
        ?.closest<HTMLElement>('[data-table-value-cell="true"]')
      if (!hit || !root.contains(hit)) return
      const nextCoordinate = hit.dataset.coordinateId
      const nextField = hit.dataset.field
      if (nextCoordinate && nextField) optionsRef.current.onSelectCell(nextCoordinate, nextField, 'range')
    }
    const tick = () => {
      if (!dragging) return
      const rect = root.getBoundingClientRect()
      let dx = 0
      let dy = 0
      if (clientX < rect.left + EDGE) dx = -Math.ceil((rect.left + EDGE - clientX) / 4)
      else if (clientX > rect.right - EDGE) dx = Math.ceil((clientX - rect.right + EDGE) / 4)
      if (clientY < rect.top + EDGE) dy = -Math.ceil((rect.top + EDGE - clientY) / 4)
      else if (clientY > rect.bottom - EDGE) dy = Math.ceil((clientY - rect.bottom + EDGE) / 4)
      if (dx || dy) {
        root.scrollBy(dx, dy)
        selectAtPoint()
      }
      frame = requestAnimationFrame(tick)
    }
    const cleanup = () => {
      document.removeEventListener('pointermove', onMove)
      document.removeEventListener('pointerup', onUp)
      document.removeEventListener('pointercancel', onUp)
      cancelAnimationFrame(frame)
      document.body.classList.remove('table-cell-range-dragging')
      cleanupRef.current = null
    }
    const onMove = (move: PointerEvent) => {
      clientX = move.clientX
      clientY = move.clientY
      if (!dragging && Math.hypot(clientX - startX, clientY - startY) >= DRAG_THRESHOLD) {
        dragging = true
        document.body.classList.add('table-cell-range-dragging')
        frame = requestAnimationFrame(tick)
      }
      if (!dragging) return
      move.preventDefault()
      selectAtPoint()
    }
    const onUp = () => cleanup()
    document.addEventListener('pointermove', onMove, { passive: false })
    document.addEventListener('pointerup', onUp)
    document.addEventListener('pointercancel', onUp)
    cleanupRef.current = cleanup
  }

  return { onPointerDown }
}
