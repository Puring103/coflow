import { useCallback, type RefObject } from 'react'
import { errorMessage, type FieldPathSegment, type FieldValue } from '../wire'
import {
  moveRecordItem,
  type RecordItemDirection,
  type VisibleRecordItem,
} from '../state/recordItemNavigation'
import { selectionEditIntentForKey } from '../state/selectionKeyboard'

interface Options {
  rootRef: RefObject<HTMLElement | null>
  selectedFieldPath: FieldPathSegment[] | null
  selectedActionPathWire: string | null
  expandedPaths: ReadonlySet<string>
  onSelectValue: (path: FieldPathSegment[]) => void
  onSelectAction: (pathWire: string | null) => void
  onToggleExpansion: (path: string, expanded: boolean) => void
  onRenderCellText?: (path: FieldPathSegment[]) => Promise<string>
  onParseCellText?: (path: FieldPathSegment[], text: string) => Promise<FieldValue>
  onWriteField?: (path: FieldPathSegment[], value: FieldValue) => Promise<unknown>
  onNotice?: (notice: string | null) => void
  onBoundary?: (edge: 'before' | 'parent') => void
}

export function useRecordItemKeyboard(options: Options) {
  const selectElement = useCallback((element: HTMLElement) => {
    const actionPath = element.dataset.addPathWire
    if (actionPath) {
      options.onSelectAction(actionPath)
      return
    }
    const path = parseWireFieldPath(element.dataset.fieldPathWire)
    if (!path) return
    options.onSelectAction(null)
    options.onSelectValue(path)
  }, [options.onSelectAction, options.onSelectValue])

  const selectFirstItem = useCallback(() => {
    const first = recordItemElements(options.rootRef.current)[0]
    if (!first) return false
    selectElement(first)
    first.scrollIntoView({ block: 'nearest' })
    return true
  }, [options.rootRef, selectElement])

  const onKeyDown = useCallback(async (event: React.KeyboardEvent) => {
    if (isNativeEditorTarget(event.target)) return
    const elements = recordItemElements(options.rootRef.current)
    if (elements.length === 0) return

    const selectedWire = options.selectedFieldPath ? JSON.stringify(options.selectedFieldPath) : null
    const current = options.selectedActionPathWire
      ? elements.find(element => element.dataset.addPathWire === options.selectedActionPathWire)
      : selectedWire
        ? elements.find(element => element.dataset.fieldPathWire === selectedWire)
        : null
    const isDirection = event.key === 'ArrowUp'
      || event.key === 'ArrowDown'
      || event.key === 'ArrowLeft'
      || event.key === 'ArrowRight'

    if (!isDirection) {
      if (!current) return
      if (current.dataset.addPathWire) {
        if (event.key === 'Enter') {
          event.preventDefault()
          current.querySelector<HTMLButtonElement>('.btn-add-item')?.click()
        }
        return
      }
      const path = parseWireFieldPath(current.dataset.fieldPathWire)
      const valueKind = current.dataset.valueKind as FieldValue['kind'] | undefined
      const editable = current.dataset.keyboardEditable === 'true'
      if (!path || !valueKind) return
      const lower = event.key.toLowerCase()
      if ((event.ctrlKey || event.metaKey) && lower === 'c' && options.onRenderCellText) {
        event.preventDefault()
        try {
          const text = await options.onRenderCellText(path)
          await navigator.clipboard.writeText(text)
          options.onNotice?.(null)
        } catch (error) {
          options.onNotice?.(`复制失败：${errorMessage(error)}`)
        }
        return
      }
      if ((event.ctrlKey || event.metaKey) && lower === 'v' && editable && options.onParseCellText && options.onWriteField) {
        event.preventDefault()
        try {
          const text = await navigator.clipboard.readText()
          const next = await options.onParseCellText(path, text)
          await options.onWriteField(path, next)
          options.onNotice?.(null)
        } catch (error) {
          options.onNotice?.(`粘贴格式不正确：${errorMessage(error)}`)
        } finally {
          requestAnimationFrame(() => options.rootRef.current?.focus({ preventScroll: true }))
        }
        return
      }
      if (!editable || !options.onWriteField) return
      const intent = selectionEditIntentForKey(
        event.key,
        event.ctrlKey || event.metaKey || event.altKey,
        valueKind,
      )
      if (!intent) return
      event.preventDefault()
      if (intent.kind === 'clear' || intent.kind === 'toggle-bool') {
        try {
          const next: FieldValue = intent.kind === 'clear'
            ? { kind: 'null' }
            : { kind: 'bool', value: current.dataset.boolValue !== 'true' }
          await options.onWriteField(path, next)
          options.onNotice?.(null)
        } catch (error) {
          options.onNotice?.(`无法编辑：${errorMessage(error)}`)
        } finally {
          requestAnimationFrame(() => options.rootRef.current?.focus({ preventScroll: true }))
        }
        return
      }
      focusRecordValueEditor(current, intent.kind === 'replace' ? intent.text : null)
      return
    }

    if (!current) {
      event.preventDefault()
      selectElement(elements[0])
      return
    }
    const items: VisibleRecordItem[] = elements.map(element => {
      const expansionPath = element.dataset.fieldPath
      return {
        id: recordItemId(element),
        depth: Number(element.dataset.depth ?? 0),
        expandable: element.classList.contains('dc-row-foldout'),
        expanded: !!expansionPath && options.expandedPaths.has(expansionPath),
      }
    })
    const result = moveRecordItem(items, recordItemId(current), event.key as RecordItemDirection)
    if (!result) return
    event.preventDefault()
    if (result.kind === 'boundary') {
      options.onBoundary?.(result.edge)
      return
    }
    const target = elements.find(element => recordItemId(element) === result.id)
    if (!target) return
    if (result.kind === 'toggle') {
      const expansionPath = target.dataset.fieldPath
      if (expansionPath) options.onToggleExpansion(expansionPath, !options.expandedPaths.has(expansionPath))
      return
    }
    selectElement(target)
    target.scrollIntoView({ block: 'nearest' })
  }, [options, selectElement])

  return { onKeyDown, selectFirstItem }
}

function recordItemElements(root: HTMLElement | null): HTMLElement[] {
  return Array.from(root?.querySelectorAll<HTMLElement>(
    '.dc-row[data-field-path-wire], .dc-row-add[data-add-path-wire]',
  ) ?? [])
}

function recordItemId(element: HTMLElement): string {
  return element.dataset.addPathWire
    ? `add:${element.dataset.addPathWire}`
    : `value:${element.dataset.fieldPathWire ?? ''}`
}

function parseWireFieldPath(raw: string | undefined): FieldPathSegment[] | null {
  if (!raw) return null
  try {
    const value = JSON.parse(raw)
    return Array.isArray(value) ? value as FieldPathSegment[] : null
  } catch {
    return null
  }
}

function focusRecordValueEditor(row: HTMLElement, replacement: string | null) {
  const editor = row.querySelector<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>(
    '.dc-row-value input:not([type="checkbox"]), .dc-row-value textarea, .dc-row-value select',
  )
  if (!editor) return
  editor.focus({ preventScroll: true })
  if (editor instanceof HTMLInputElement && editor.classList.contains('searchable-select')) {
    try { editor.showPicker() } catch { /* Typing still searches. */ }
    return
  }
  if (editor instanceof HTMLSelectElement) {
    try { editor.showPicker() } catch { /* Native picker support varies by WebView. */ }
    return
  }
  if (replacement !== null) {
    const prototype = editor instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype
    Object.getOwnPropertyDescriptor(prototype, 'value')?.set?.call(editor, replacement)
    editor.dispatchEvent(new Event('input', { bubbles: true }))
    editor.setSelectionRange(replacement.length, replacement.length)
    return
  }
  editor.select()
}

function isNativeEditorTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  return target.isContentEditable
    || target.tagName === 'INPUT'
    || target.tagName === 'TEXTAREA'
    || target.tagName === 'SELECT'
    || target.tagName === 'BUTTON'
}
