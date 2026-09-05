/**
 * 选择变化前同步结束当前数据编辑，让 blur 仍使用旧渲染捕获的写入目标。
 * 未完成的菜单和结构化操作不在这里提交，由目标组件卸载时直接取消。
 */
export function finishActiveDataEdit(): void {
  const active = document.activeElement
  if (!(active instanceof HTMLElement)) return
  if (!active.matches('.gn-key-editor, .dc-input, .searchable-select')) return
  active.blur()
}
