import { useLayoutEffect, type RefObject } from 'react'
import { planDiffRowAlignment, type DiffRowSpacing } from '../state/diffRowAlignment'

/**
 * 保留共用记录组件的布局，仅为对比两侧补齐空白和行高。
 * 新增/删除子项、长文本换行及手动展开后，后续相同路径仍在同一水平线上。
 */
export function useDiffRowAlignment(rootRef: RefObject<HTMLDivElement | null>, record: unknown) {
  useLayoutEffect(() => {
    const root = rootRef.current
    if (!root) return
    let frame = 0
    const rows = (pane: Element) => [...pane.querySelectorAll<HTMLElement>('.dc-row[data-field-path]')]
    const align = () => {
      frame = 0
      const panes = [...root.querySelectorAll<HTMLElement>('.rv-main')]
      if (panes.length !== 2) return
      const [left, right] = panes
      const leftRows = rows(left)
      const rightRows = rows(right)
      const inspectors = panes.map(pane => pane.querySelector<HTMLElement>('.dc-inspector'))
      for (const inspector of inspectors) if (inspector) inspector.style.minHeight = ''
      for (const row of [...leftRows, ...rightRows]) {
        row.style.marginTop = ''
        row.style.minHeight = ''
      }
      const measure = (items: HTMLElement[], pane: HTMLElement) => {
        const origin = pane.getBoundingClientRect().top - pane.scrollTop
        return items.map(row => {
          const bounds = row.getBoundingClientRect()
          return { path: row.dataset.fieldPath!, top: bounds.top - origin, height: bounds.height }
        })
      }
      // 先批量测量，再统一写样式，避免每个字段都触发一次同步布局。
      const spacing = planDiffRowAlignment(measure(leftRows, left), measure(rightRows, right))
      const apply = (items: HTMLElement[], plan: DiffRowSpacing[]) => {
        for (const { index, before, height } of plan) {
          items[index].style.marginTop = before > 0.5 ? `${before}px` : ''
          items[index].style.minHeight = `${height}px`
        }
      }
      apply(leftRows, spacing.left)
      apply(rightRows, spacing.right)
      const height = Math.max(...inspectors.map(inspector => inspector?.getBoundingClientRect().height ?? 0))
      for (const inspector of inspectors) if (inspector) inspector.style.minHeight = `${height}px`
    }
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(align)
    }
    const resize = new ResizeObserver(schedule)
    resize.observe(root)
    const observeInspectors = () => {
      for (const inspector of root.querySelectorAll('.dc-inspector')) resize.observe(inspector)
      schedule()
    }
    const mutations = new MutationObserver(observeInspectors)
    mutations.observe(root, { childList: true, subtree: true, characterData: true })
    const syncScroll = (event: Event) => {
      const source = event.target
      if (!(source instanceof HTMLElement) || !source.matches('.rv-main')) return
      for (const pane of root.querySelectorAll<HTMLElement>('.rv-main')) {
        if (pane !== source && Math.abs(pane.scrollTop - source.scrollTop) > 0.5) pane.scrollTop = source.scrollTop
      }
    }
    root.addEventListener('scroll', syncScroll, true)
    align()
    observeInspectors()
    return () => {
      resize.disconnect()
      mutations.disconnect()
      cancelAnimationFrame(frame)
      root.removeEventListener('scroll', syncScroll, true)
    }
  }, [rootRef, record])
}
