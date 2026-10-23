import type { FieldPathSegment } from '../wire'

export function cssEscape(value: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(value)
  return value.replace(/["\\]/g, '\\$&')
}

export function parseWireFieldPath(raw: string | undefined): FieldPathSegment[] | null {
  if (!raw) return null
  try {
    const value: unknown = JSON.parse(raw)
    return Array.isArray(value) ? value as FieldPathSegment[] : null
  } catch {
    return null
  }
}

export function isNativeEditorTarget(
  target: EventTarget | null,
  includeButton = true,
): boolean {
  if (!(target instanceof HTMLElement)) return false
  return target.isContentEditable
    || target.tagName === 'INPUT'
    || target.tagName === 'TEXTAREA'
    || target.tagName === 'SELECT'
    || (includeButton && target.tagName === 'BUTTON')
}

/** 键盘从网格进入正文时，通过原生 setter 通知 React，保留现有输入节点。 */
export function replaceInputText(editor: HTMLInputElement | HTMLTextAreaElement, text: string): void {
  const prototype = editor instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype
  Object.getOwnPropertyDescriptor(prototype, 'value')?.set?.call(editor, text)
  editor.dispatchEvent(new Event('input', { bubbles: true }))
  editor.setSelectionRange(text.length, text.length)
}
