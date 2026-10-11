import { useEffect, useRef } from 'react'
import { Icon } from './Icon'
import './HelpDialog.css'

interface Props {
  open: boolean
  onClose: () => void
}

export function HelpDialog({ open, onClose }: Props) {
  const boxRef = useRef<HTMLDivElement>(null)
  const returnFocusRef = useRef<HTMLElement | null>(null)

  useEffect(() => {
    if (!open) return
    returnFocusRef.current = document.activeElement as HTMLElement | null
    const box = boxRef.current
    box?.querySelector<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
    )?.focus()

    const trapFocus = (event: KeyboardEvent) => {
      if (event.key !== 'Tab' || !box) return
      const nodes = Array.from(box.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      )).filter(element => !element.hasAttribute('disabled'))
      if (nodes.length === 0) return
      const first = nodes[0]
      const last = nodes[nodes.length - 1]
      const active = document.activeElement as HTMLElement | null
      if (event.shiftKey ? active === first || !box.contains(active) : active === last || !box.contains(active)) {
        event.preventDefault()
        ;(event.shiftKey ? last : first).focus()
      }
    }

    window.addEventListener('keydown', trapFocus)
    return () => {
      window.removeEventListener('keydown', trapFocus)
      returnFocusRef.current?.focus()
      returnFocusRef.current = null
    }
  }, [open])

  if (!open) return null
  return (
    <div className="help-overlay" onClick={onClose}>
      <div
        className="help-box"
        ref={boxRef}
        role="dialog"
        aria-modal="true"
        aria-label="键盘快捷键"
        onClick={event => event.stopPropagation()}
      >
        <h3><Icon name="help" size={16} />键盘快捷键</h3>
        <table>
          <tbody>
            <tr><th colSpan={2}>全局</th></tr>
            <tr><td>Alt+←</td><td>后退</td></tr>
            <tr><td>Alt+→</td><td>前进</td></tr>
            <tr><td>Ctrl+Z</td><td>撤销编辑</td></tr>
            <tr><td>Ctrl+Y / Ctrl+Shift+Z</td><td>重做编辑</td></tr>
            <tr><td>?</td><td>显示/隐藏帮助</td></tr>
            <tr><td>Esc</td><td>关闭弹窗</td></tr>
            <tr><th colSpan={2}>表格 / 记录导航</th></tr>
            <tr><td>↑ ↓ ← →</td><td>移动选中单元格</td></tr>
            <tr><td>Shift+↑ / Shift+↓</td><td>扩展多行记录选择</td></tr>
            <tr><td>Shift+↑↓←→（值模式）</td><td>扩展选中单元格范围</td></tr>
            <tr><td>Ctrl+A</td><td>全选记录 / 全选单元格范围</td></tr>
            <tr><td>Enter</td><td>打开检视器 / 切换布尔值</td></tr>
            <tr><th colSpan={2}>单元格编辑</th></tr>
            <tr><td>F2</td><td>编辑当前单元格</td></tr>
            <tr><td>任意可打印字符</td><td>替换输入（直接开始编辑）</td></tr>
            <tr><td>Delete</td><td>重置为默认值（集合类型清空）</td></tr>
            <tr><th colSpan={2}>复制 / 粘贴</th></tr>
            <tr><td>Ctrl+C</td><td>复制选中单元格（CFD）</td></tr>
            <tr><td>Ctrl+X</td><td>剪切（复制后清空）</td></tr>
            <tr><td>Ctrl+V</td><td>粘贴</td></tr>
            <tr><td>Shift+Ctrl+V</td><td>追加粘贴（array 目标）</td></tr>
            <tr><th colSpan={2}>Ctrl+鼠标滚轮</th></tr>
            <tr><td>Ctrl+滚轮</td><td>缩放表格</td></tr>
          </tbody>
        </table>
        <div className="help-actions">
          <button className="btn btn-outlined" onClick={onClose}>关闭</button>
        </div>
      </div>
    </div>
  )
}
