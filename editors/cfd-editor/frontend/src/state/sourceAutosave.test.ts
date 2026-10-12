import { describe, it, expect, vi } from 'vitest'
import { SourceAutosave, registerSourceSave, deferSourceNavigation } from './sourceAutosave'

function fixture(write = vi.fn(async (_text: string, _base: string): Promise<void> => undefined)) {
  const read = vi.fn(async () => 'disk')
  const saved = vi.fn(async () => undefined)
  const model = new SourceAutosave({ read, write, saved, message: String })
  model.receiveDisk('base\r\n')
  return { model, read, write, saved }
}
describe('source save lifecycle', () => {
  it('serializes writes and retains input typed during an in-flight save', async () => {
    let release!: () => void
    const first = new Promise<void>(resolve => { release = resolve })
    const write = vi.fn(async (_text: string, _base: string): Promise<void> => first)
    const { model } = fixture(write)
    model.edit('one')
    const saving = model.flush()
    expect(model.flush()).toBe(saving)
    model.edit('two')
    release()
    expect(await saving).toBe(true)
    expect(write.mock.calls).toEqual([['one', 'base\r\n'], ['two', 'one']])
    expect(model.dirty).toBe(false)
    expect(model.state.text).toBe('two')
  })
  it('keeps local input when disk changes and checks the confirmed override base', async () => {
    const { model, write } = fixture()
    model.edit('local')
    model.receiveDisk('external')
    expect(model.state.text).toBe('local')
    expect(await model.flush()).toBe(false)
    expect(write).not.toHaveBeenCalled()
    expect(await model.keepLocal()).toBe(true)
    expect(write).toHaveBeenCalledWith('local', 'external')
  })
  it('keeps input and blocks navigation after write failure', async () => {
    const { model } = fixture(vi.fn(async () => { throw new Error('conflict') }))
    model.edit('local')
    const navigate = vi.fn()
    const unregister = registerSourceSave({ pending: () => model.dirty, flush: model.flush })
    expect(deferSourceNavigation(navigate)).toBe(true)
    expect(await model.flush()).toBe(false)
    expect(navigate).not.toHaveBeenCalled()
    expect(model.state.text).toBe('local')
    expect(model.state.conflict).toBe('disk')
    unregister()
  })
  it('does not repeat a completed disk write after publication failure', async () => {
    const { model, write, saved } = fixture()
    saved.mockRejectedValue(new Error('refresh failed'))
    model.edit('local')
    expect(await model.flush()).toBe(false)
    expect(model.state.base).toBe('local')
    expect(await model.flush()).toBe(true)
    expect(write).toHaveBeenCalledTimes(1)
  })
  it('accepts disk content only after explicit conflict resolution', () => {
    const { model } = fixture()
    model.edit('local')
    model.receiveDisk('external\r\n')
    model.useDisk()
    expect(model.state.text).toBe('external\n')
    expect(model.state.base).toBe('external\r\n')
    expect(model.dirty).toBe(false)
  })
  it('leaves the source view only after a successful save', async () => {
    let release!: () => void
    const wait = new Promise<void>(resolve => { release = resolve })
    const { model } = fixture(vi.fn(async (_text: string, _base: string): Promise<void> => wait))
    model.edit('local')
    const navigate = vi.fn()
    const unregister = registerSourceSave({ pending: () => model.dirty, flush: model.flush })
    expect(deferSourceNavigation(navigate)).toBe(true)
    expect(navigate).not.toHaveBeenCalled()
    release()
    await model.flush()
    await Promise.resolve()
    expect(navigate).toHaveBeenCalledTimes(1)
    unregister()
  })

})
