/** 多个单元格共用观察器，单次通知先读完所有尺寸再更新 React 状态。 */
const callbacks = new Map<Element, Set<() => void>>()
let observer: ResizeObserver | undefined
export function observeCellSize(elements: Element[], callback: () => void): () => void {
  observer ??= new ResizeObserver(entries => {
    const pending = new Set<() => void>()
    entries.forEach(entry => callbacks.get(entry.target)?.forEach(callback => pending.add(callback)))
    pending.forEach(callback => callback())
  })
  elements.forEach(element => {
    let listeners = callbacks.get(element)
    if (!listeners) { callbacks.set(element, listeners = new Set()); observer!.observe(element) }
    listeners.add(callback)
  })
  return () => {
    elements.forEach(element => {
      const listeners = callbacks.get(element)!
      listeners.delete(callback)
      if (!listeners.size) { observer!.unobserve(element); callbacks.delete(element) }
    })
    if (!callbacks.size) { observer!.disconnect(); observer = undefined }
  }
}
