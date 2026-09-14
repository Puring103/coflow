import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react'

const STORAGE_KEY = 'cfd-editor-sidebar-w'
const clampWidth = (width: number) => Math.min(480, Math.max(160, width))

export function useSidebarWidth(defaultWidth = 220) {
  const [width, setWidth] = useState(() => {
    try {
      const stored = Number(localStorage.getItem(STORAGE_KEY))
      return Number.isFinite(stored) && stored > 0 ? clampWidth(stored) : defaultWidth
    } catch { return defaultWidth }
  })
  const [dragging, setDragging] = useState(false)
  const widthRef = useRef(width)
  const dragCleanupRef = useRef<(() => void) | null>(null)
  widthRef.current = width

  useEffect(() => {
    document.documentElement.style.setProperty('--sidebar-w', `${width}px`)
  }, [width])

  useEffect(() => () => dragCleanupRef.current?.(), [])

  const resizeBy = useCallback((delta: number) => {
    const next = clampWidth(widthRef.current + delta)
    widthRef.current = next
    setWidth(next)
    try { localStorage.setItem(STORAGE_KEY, String(next)) } catch { /* WebView 存储不可用 */ }
  }, [])

  const onSplitterMouseDown = useCallback((event: ReactMouseEvent) => {
    event.preventDefault()
    setDragging(true)
    const startX = event.clientX
    const startWidth = width
    let finalWidth = startWidth
    const onMove = (moveEvent: MouseEvent) => {
      finalWidth = clampWidth(startWidth + moveEvent.clientX - startX)
      widthRef.current = finalWidth
      setWidth(finalWidth)
    }
    const cleanup = () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
      dragCleanupRef.current = null
    }
    const onUp = () => {
      cleanup()
      setDragging(false)
      try { localStorage.setItem(STORAGE_KEY, String(finalWidth)) } catch { /* WebView 存储不可用 */ }
    }
    dragCleanupRef.current?.()
    dragCleanupRef.current = cleanup
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [width])

  return { width, dragging, resizeBy, onSplitterMouseDown }
}
