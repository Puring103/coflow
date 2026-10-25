import { useEffect, useRef } from 'react'
import type { ComponentProps } from 'react'
import { PluginContributionMount } from '../plugins'
import { Icon } from './Icon'

type MountProps = ComponentProps<typeof PluginContributionMount>

/** 原生模态窗口负责焦点约束与恢复；隐藏时保留搜索结果，方便连续查找。 */
export function ProjectSearchDialog({ open, onClose, contribution, context }: {
  open: boolean
  onClose: () => void
  contribution: MountProps['contribution']
  context: MountProps['context']
}) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const dialog = ref.current
    if (!dialog) return
    if (!open) { if (dialog.open) dialog.close(); return }
    if (!dialog.open) dialog.showModal()
    const frame = requestAnimationFrame(() => dialog.querySelector<HTMLInputElement>('[data-built-in-project-search-input]')?.focus())
    return () => cancelAnimationFrame(frame)
  }, [open])
  return <dialog ref={ref} className="project-search-dialog" aria-labelledby="project-search-title"
    onKeyDownCapture={event => {
      if (event.key === 'Escape' && !event.nativeEvent.isComposing) { event.preventDefault(); event.stopPropagation(); onClose() }
    }}
    onCancel={onClose} onClose={() => { if (!ref.current?.open) onClose() }} onClick={event => {
      if (event.target !== event.currentTarget) return
      const rect = event.currentTarget.getBoundingClientRect()
      if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) onClose()
    }}>
    <header className="project-search-heading"><Icon name="search" size={17} /><strong id="project-search-title">全局搜索</strong>
      <span>查找项目中的记录与字段值</span><button className="btn btn-icon" title="关闭（Esc）" aria-label="关闭全局搜索" onClick={onClose}><Icon name="close" size={16} /></button></header>
    <PluginContributionMount contribution={contribution} context={context} className="project-search-content" />
    <footer className="project-search-footer"><span>↑ ↓ 选择结果 · Enter 打开</span><span>Esc 关闭</span></footer>
  </dialog>
}
