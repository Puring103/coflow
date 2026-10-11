export const normalizeSource = (text: string) => text.replace(/\r\n?/g, '\n')

export interface SourceSaveState {
  loaded: boolean
  text: string
  base: string
  saving: boolean
  conflict: string | null
  error: string | null
}

/** 只持有当前编辑文档；写入串行化，成功后推进磁盘基线。 */
export class SourceAutosave<T> {
  state: SourceSaveState = { loaded: false, text: '', base: '', saving: false, conflict: null, error: null }
  private listeners = new Set<() => void>()
  private flight: Promise<boolean> | null = null
  constructor(private io: {
    read: () => Promise<string>
    write: (text: string, expected: string) => Promise<T>
    saved: (result: T) => Promise<void> | void
    message: (error: unknown) => string
  }) {}
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  snapshot = () => this.state
  get dirty() { return this.state.loaded && this.state.text !== normalizeSource(this.state.base) }
  private update(patch: Partial<SourceSaveState>) {
    this.state = { ...this.state, ...patch }
    this.listeners.forEach(listener => listener())
  }
  receiveDisk(raw: string) {
    if (this.state.saving) return
    if (this.dirty && raw !== this.state.base) {
      this.update({ conflict: raw, error: '文件已被其他操作修改，自动保存已暂停' })
    } else if (!this.dirty) {
      this.update({ loaded: true, base: raw, text: normalizeSource(raw), conflict: null })
    }
  }
  edit(text: string) { this.update({ text, error: this.state.conflict === null ? null : this.state.error }) }
  fail(error: unknown) { this.update({ error: this.io.message(error) }) }
  useDisk() {
    if (this.state.conflict === null) return
    const raw = this.state.conflict
    this.update({ base: raw, text: normalizeSource(raw), conflict: null, error: null })
  }
  async keepLocal() {
    if (this.state.conflict === null) return false
    // 用户确认的磁盘版本成为下一次写入的期望值；再次变化仍然冲突。
    this.update({ base: this.state.conflict, conflict: null, error: null })
    return this.flush()
  }
  flush = (): Promise<boolean> => {
    if (this.flight) return this.flight
    if (this.state.conflict !== null) return Promise.resolve(false)
    if (!this.dirty) return Promise.resolve(true)
    this.flight = this.save().finally(() => { this.flight = null })
    return this.flight
  }
  private async save(): Promise<boolean> {
    this.update({ saving: true, error: null })
    try {
      while (this.dirty) {
        const text = this.state.text
        const result = await this.io.write(text, this.state.base)
        this.update({ base: text })
        // 保存后的 UI 刷新失败不能被当成磁盘写入失败或再次写入。
        try { await this.io.saved(result) } catch (error) { this.fail(error); return false }
      }
      return true
    } catch (error) {
      this.fail(error)
      try {
        const disk = await this.io.read()
        if (disk !== this.state.base) this.update({ conflict: disk })
      } catch { /* 读取失败时保留当前内容与写入错误。 */ }
      return false
    } finally { this.update({ saving: false }) }
  }
}

// 只注册当前视图的保存能力，不保存跨页面文本。
let active: { pending: () => boolean; flush: () => Promise<boolean> } | null = null
export function registerSourceSave(value: NonNullable<typeof active>) {
  active = value
  return () => { if (active === value) active = null }
}
let navigation = 0
export function deferSourceNavigation(action: () => void): boolean {
  if (!active?.pending()) return false
  const current = active
  const request = ++navigation
  void current.flush().then(ok => {
    if (ok && active === current && request === navigation) action()
  })
  return true
}
export function afterSourceSave(action: () => void) {
  if (!deferSourceNavigation(action)) action()
}

export async function flushSourceChanges(): Promise<boolean> {
  return active ? active.flush() : true
}
