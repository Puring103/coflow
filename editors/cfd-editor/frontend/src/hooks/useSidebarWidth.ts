import { useCallback, useEffect, useState, type MouseEvent as ReactMouseEvent } from 'react'

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

  useEffect(() => {
    document.documentElement.style.setProperty('--sidebar-w', `${width}px`)
  }, [width])

  const resizeBy = useCallback((delta: number) => {
    setWidth(current => {
      const next = clampWidth(current + delta)
      try { localStorage.setItem(STORAGE_KEY, String(next)) } catch { /* WebView 存储不可用 */ }
      return next
    })
  }, [])

  const onSplitterMouseDown = useCallback((event: ReactMouseEvent) => {
    event.preventDefault()
    setDragging(true)
    const startX = event.clientX
    const startWidth = width
    let finalWidth = startWidth
    const onMove = (moveEvent: MouseEvent) => {
      finalWidth = clampWidth(startWidth + moveEvent.clientX - startX)
      setWidth(finalWidth)
    }
    const onUp = () => {
      setDragging(false)
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
      try { localStorage.setItem(STORAGE_KEY, String(finalWidth)) } catch { /* WebView 存储不可用 */ }
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [width])

  return { width, dragging, resizeBy, onSplitterMouseDown }
}
