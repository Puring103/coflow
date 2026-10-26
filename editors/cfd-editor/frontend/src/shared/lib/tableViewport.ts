/** 只滚动表格自身，并为冻结键列、表头及滚动条保留可见区域。 */
export function revealTableCell(scroller: HTMLElement, cell: HTMLElement, bounds = cell.getBoundingClientRect()) {
  const viewport = scroller.getBoundingClientRect()
  const frozen = scroller.querySelector('thead .sticky-key-column')?.getBoundingClientRect()
  const headers = [...scroller.querySelectorAll('thead th')].map(element => element.getBoundingClientRect().bottom)
  const left = cell.classList.contains('sticky-key-column') ? viewport.left : Math.max(viewport.left, frozen?.right ?? viewport.left)
  const top = Math.max(viewport.top, ...headers)
  const right = viewport.left + scroller.clientWidth
  const bottom = viewport.top + scroller.clientHeight
  const dx = bounds.left < left ? bounds.left - left : bounds.right > right ? Math.min(bounds.right - right, bounds.left - left) : 0
  const dy = bounds.top < top ? bounds.top - top : bounds.bottom > bottom ? Math.min(bounds.bottom - bottom, bounds.top - top) : 0
  if (dx || dy) scroller.scrollBy({ left: dx, top: dy, behavior: 'instant' })
}
