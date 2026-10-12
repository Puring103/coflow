import { afterEach, describe, expect, it, vi } from 'vitest'
import { historyShortcutFor } from './editorShortcuts'

class StubHtmlElement {
  isContentEditable = false

  constructor(
    readonly tagName: string,
    readonly type = '',
  ) {}
}

function keyboardEvent(
  key: string,
  target: StubHtmlElement,
  options: { shiftKey?: boolean } = {},
): KeyboardEvent {
  return {
    key,
    target,
    ctrlKey: true,
    metaKey: false,
    shiftKey: options.shiftKey ?? false,
  } as unknown as KeyboardEvent
}

afterEach(() => vi.unstubAllGlobals())

describe('historyShortcutFor', () => {
  it('routes undo from a select that already committed its value', () => {
    vi.stubGlobal('HTMLElement', StubHtmlElement)

    expect(historyShortcutFor(keyboardEvent('z', new StubHtmlElement('SELECT')))).toBe('undo')
  })

  it('routes redo from a checkbox that already committed its value', () => {
    vi.stubGlobal('HTMLElement', StubHtmlElement)

    expect(historyShortcutFor(keyboardEvent('y', new StubHtmlElement('INPUT', 'checkbox')))).toBe('redo')
  })

  it('leaves undo with an active text draft to the native input history', () => {
    vi.stubGlobal('HTMLElement', StubHtmlElement)

    expect(historyShortcutFor(keyboardEvent('z', new StubHtmlElement('INPUT', 'text')))).toBeNull()
  })
})
