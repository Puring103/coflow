import type { FileRecords } from '../bindings/FileRecords'
import type { RecordRow } from '../bindings/RecordRow'
import type {
  BuiltInPluginDefinition,
  EditorPluginHost,
  PluginSearchHit,
  PluginSearchMode,
  PluginSidebarContext,
  PluginSnapshot,
} from '../plugins'
import type { FieldValue } from '../wire'
import { summaryOf } from '../value/fieldValue'
import './projectSearch.css'

const SIDEBAR_ID = 'project-search'
const COMMAND_ID = 'open-project-search'
const RESULT_LIMIT = 200

type SearchResult = PluginSnapshot<{ hits: PluginSearchHit[]; truncated: boolean }>

interface SearchTypeGroup {
  actualType: string
  hits: PluginSearchHit[]
}

interface SearchFileGroup {
  filePath: string
  hits: PluginSearchHit[]
  types: SearchTypeGroup[]
}

export const projectSearchPlugin: BuiltInPluginDefinition = {
  id: 'project-search',
  name: '项目搜索',
  description: '跨项目数据文件搜索记录和字段值',
  version: '1.0.0',
  activate(host) {
    host.register.sidebar({
      id: SIDEBAR_ID,
      title: '全局搜索',
      icon: 'search',
      mount(context, outlet) {
        return mountProjectSearch(host, context, outlet.element)
      },
    })
    host.register.command({
      id: COMMAND_ID,
      title: '打开全局搜索',
      run() {
        host.register.openSidebar(SIDEBAR_ID)
        // 侧栏由 React 在命令返回后挂载，下一帧再把焦点交给搜索框。
        requestAnimationFrame(() => {
          document.querySelector<HTMLInputElement>('[data-built-in-project-search-input]')?.focus()
        })
      },
    })
    host.register.keybinding({ command: COMMAND_ID, key: 'Mod+Shift+F' })
  },
}

function mountProjectSearch(
  host: EditorPluginHost,
  initialContext: PluginSidebarContext,
  outlet: HTMLElement,
) {
  let context = initialContext
  let query = ''
  let mode: PluginSearchMode = 'key'
  let results: SearchResult | null = null
  let busy = false
  let failure: string | null = null
  let selectedIndex = -1
  let request = 0
  let timeout: number | null = null
  const collapsed = new Set<string>()

  const root = element('div', 'project-search-pane')
  const header = element('div', 'sidebar-header')
  const title = element('span')
  title.textContent = '全局搜索'
  const total = element('span', 'project-search-total')
  header.append(title, total)

  const controls = element('div', 'pane-search-wrap')
  const searchLabel = element('label', 'pane-search')
  const searchMark = element('span', 'project-search-mark')
  searchMark.setAttribute('aria-hidden', 'true')
  const input = document.createElement('input')
  input.placeholder = '输入 Key 或字段值...'
  input.setAttribute('aria-label', '跨文件搜索')
  input.dataset.builtInProjectSearchInput = ''
  const clear = element('button', 'pane-search-clear') as HTMLButtonElement
  clear.type = 'button'
  clear.setAttribute('aria-label', '清除全局搜索')
  clear.textContent = '×'
  searchLabel.append(searchMark, input, clear)

  const modes = element('div', 'project-search-modes')
  modes.setAttribute('role', 'group')
  modes.setAttribute('aria-label', '搜索范围')
  const keyMode = modeButton('Key', 'key')
  const fullTextMode = modeButton('全文', 'full_text')
  modes.append(keyMode, fullTextMode)
  controls.append(searchLabel, modes)

  const resultList = element('div', 'project-search-results')
  resultList.setAttribute('role', 'tree')
  resultList.setAttribute('aria-label', '全局搜索结果')
  root.append(header, controls, resultList)
  outlet.replaceChildren(root)

  function modeButton(label: string, nextMode: PluginSearchMode): HTMLButtonElement {
    const button = document.createElement('button')
    button.type = 'button'
    button.textContent = label
    button.addEventListener('click', () => {
      if (mode === nextMode) return
      mode = nextMode
      scheduleSearch()
      render()
    })
    return button
  }

  function scheduleSearch(): void {
    const currentRequest = ++request
    selectedIndex = -1
    failure = null
    if (timeout !== null) window.clearTimeout(timeout)
    timeout = null
    if (!context.identity || !query.trim()) {
      results = null
      busy = false
      render()
      return
    }
    busy = true
    render()
    timeout = window.setTimeout(async () => {
      timeout = null
      try {
        const next = await host.data.searchRecords(query, { mode, limit: RESULT_LIMIT })
        if (request !== currentRequest) return
        results = next
      } catch (error) {
        if (request !== currentRequest) return
        failure = error instanceof Error ? error.message : String(error)
        results = null
      } finally {
        if (request === currentRequest) {
          busy = false
          render()
        }
      }
    }, 150)
  }

  function render(): void {
    total.textContent = results ? `${results.data.hits.length} 条` : ''
    clear.hidden = query.length === 0
    keyMode.classList.toggle('active', mode === 'key')
    keyMode.setAttribute('aria-pressed', String(mode === 'key'))
    fullTextMode.classList.toggle('active', mode === 'full_text')
    fullTextMode.setAttribute('aria-pressed', String(mode === 'full_text'))
    resultList.replaceChildren()

    if (!context.identity) {
      resultList.append(state('请先打开项目'))
      return
    }
    if (failure) {
      const message = state(`搜索失败：${failure}`)
      message.classList.add('error')
      message.setAttribute('role', 'alert')
      resultList.append(message)
      return
    }
    if (busy && !results) {
      resultList.append(state('正在搜索...'))
      return
    }
    if (query && results?.data.hits.length === 0) {
      resultList.append(state(`没有找到“${query}”`))
      return
    }
    if (!results) return

    for (const fileGroup of groupSearchHits(results.data.hits)) {
      const fileKey = `file:${fileGroup.filePath}`
      const fileContainer = element('div', 'project-search-file')
      fileContainer.append(groupButton(
        fileGroup.filePath,
        fileGroup.hits.length,
        'file',
        fileKey,
        () => render(),
      ))
      if (!collapsed.has(fileKey)) {
        for (const typeGroup of fileGroup.types) {
          const typeKey = `type:${fileGroup.filePath}:${typeGroup.actualType}`
          const typeContainer = element('div', 'project-search-type')
          typeContainer.append(groupButton(
            typeGroup.actualType,
            typeGroup.hits.length,
            'type',
            typeKey,
            () => render(),
          ))
          if (!collapsed.has(typeKey)) {
            for (const hit of typeGroup.hits) {
              const index = results.data.hits.indexOf(hit)
              typeContainer.append(hitButton(hit, index))
            }
          }
          fileContainer.append(typeContainer)
        }
      }
      resultList.append(fileContainer)
    }
    if (results.data.truncated) {
      resultList.append(state(`已显示前 ${RESULT_LIMIT} 条，请缩小搜索范围`, 'project-search-limit'))
    }
    if (busy) resultList.append(state('正在更新...', 'project-search-updating'))
  }

  function groupButton(
    label: string,
    count: number,
    kind: 'file' | 'type',
    key: string,
    rerender: () => void,
  ): HTMLButtonElement {
    const button = element('button', `project-search-group ${kind}`) as HTMLButtonElement
    button.type = 'button'
    button.setAttribute('aria-expanded', String(!collapsed.has(key)))
    const chevron = element('span', 'project-search-chevron')
    chevron.textContent = collapsed.has(key) ? '›' : '⌄'
    const text = element('span')
    text.textContent = label
    text.title = label
    const amount = element('b')
    amount.textContent = String(count)
    button.append(chevron, text, amount)
    button.addEventListener('click', () => {
      if (collapsed.has(key)) collapsed.delete(key)
      else collapsed.add(key)
      rerender()
    })
    return button
  }

  function hitButton(hit: PluginSearchHit, index: number): HTMLButtonElement {
    const button = element(
      'button',
      `project-search-hit${selectedIndex === index ? ' selected' : ''}`,
    ) as HTMLButtonElement
    button.type = 'button'
    button.dataset.projectSearchIndex = String(index)
    button.setAttribute('role', 'treeitem')
    const key = element('span', 'project-search-hit-key')
    key.textContent = hit.coordinate.key
    button.append(key)
    if (hit.preview) {
      const preview = element('span', 'project-search-hit-preview')
      preview.textContent = hit.preview
      button.append(preview)
    }
    button.addEventListener('focus', () => { selectedIndex = index })
    button.addEventListener('click', () => openHit(hit))
    button.addEventListener('keydown', event => {
      if (event.key === 'ArrowDown') {
        event.preventDefault()
        focusResult(index + 1)
      } else if (event.key === 'ArrowUp') {
        event.preventDefault()
        if (index === 0) input.focus()
        else focusResult(index - 1, -1)
      } else if (event.key === 'Escape') {
        event.preventDefault()
        input.focus()
      }
    })
    return button
  }

  function openHit(hit: PluginSearchHit): void {
    context.openRecord(hit.filePath, hit.coordinate, hit.fieldPath)
  }

  function focusResult(index: number, direction = 1): void {
    const count = results?.data.hits.length ?? 0
    if (count === 0) return
    requestAnimationFrame(() => {
      let next = Math.max(0, Math.min(index, count - 1))
      while (next >= 0 && next < count) {
        const target = resultList.querySelector<HTMLButtonElement>(`[data-project-search-index="${next}"]`)
        if (target) {
          selectedIndex = next
          target.focus({ preventScroll: true })
          return
        }
        next += direction
      }
    })
  }

  input.addEventListener('input', () => {
    query = input.value
    scheduleSearch()
  })
  input.addEventListener('keydown', event => {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      focusResult(selectedIndex < 0 ? 0 : selectedIndex)
    } else if (event.key === 'Enter' && results?.data.hits[0]) {
      event.preventDefault()
      openHit(results.data.hits[0])
    } else if (event.key === 'Escape') {
      event.preventDefault()
      if (query) {
        query = ''
        input.value = ''
        scheduleSearch()
      } else {
        context.closeSidebar()
      }
    }
  })
  clear.addEventListener('click', () => {
    query = ''
    input.value = ''
    scheduleSearch()
    input.focus()
  })
  render()

  return {
    update(nextContext: PluginSidebarContext) {
      const previous = context.identity
      context = nextContext
      if (previous?.sessionId !== nextContext.identity?.sessionId) {
        // 项目切换时旧结果不能短暂显示在新项目上下文中。
        results = null
        collapsed.clear()
      }
      if (
        previous?.sessionId !== nextContext.identity?.sessionId
        || previous?.revision !== nextContext.identity?.revision
      ) scheduleSearch()
      else render()
    },
    dispose() {
      request += 1
      if (timeout !== null) window.clearTimeout(timeout)
      outlet.replaceChildren()
    },
  }
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
): HTMLElementTagNameMap[K] {
  const value = document.createElement(tag)
  if (className) value.className = className
  return value
}

function state(text: string, className = 'project-search-state'): HTMLDivElement {
  const value = element('div', className)
  value.textContent = text
  return value
}

export function groupSearchHits(hits: readonly PluginSearchHit[]): SearchFileGroup[] {
  const files = new Map<string, Map<string, PluginSearchHit[]>>()
  for (const hit of hits) {
    const types = files.get(hit.filePath) ?? new Map<string, PluginSearchHit[]>()
    if (!files.has(hit.filePath)) files.set(hit.filePath, types)
    const typeHits = types.get(hit.coordinate.actual_type) ?? []
    if (!types.has(hit.coordinate.actual_type)) types.set(hit.coordinate.actual_type, typeHits)
    typeHits.push(hit)
  }
  return Array.from(files, ([filePath, types]) => {
    const typeGroups = Array.from(types, ([actualType, typeHits]) => ({ actualType, hits: typeHits }))
    return { filePath, hits: typeGroups.flatMap(group => group.hits), types: typeGroups }
  })
}

export function searchMockRecords(
  files: Record<string, FileRecords>,
  sessionId: number,
  revision: number,
  query: string,
  mode: PluginSearchMode,
  limit: number,
): PluginSnapshot<{ hits: PluginSearchHit[]; truncated: boolean }> {
  const normalized = query.trim().toLowerCase()
  if (!normalized || limit <= 0) return { sessionId, revision, data: { hits: [], truncated: false } }
  const hits: PluginSearchHit[] = []
  for (const filePath of Object.keys(files).sort()) {
    for (const record of files[filePath].records) {
      const keyMatches = record.coordinate.key.toLowerCase().includes(normalized)
      const fieldMatch = mode === 'full_text' && !keyMatches
        ? firstFieldMatch(record, normalized)
        : null
      if (!keyMatches && !fieldMatch) continue
      if (hits.length === limit) return { sessionId, revision, data: { hits, truncated: true } }
      hits.push({
        filePath,
        coordinate: record.coordinate,
        fieldPath: fieldMatch?.fieldPath ?? null,
        preview: fieldMatch?.preview ?? null,
      })
    }
  }
  return { sessionId, revision, data: { hits, truncated: false } }
}

function firstFieldMatch(record: RecordRow, query: string): { fieldPath: string; preview: string } | null {
  for (const field of record.fields) {
    if (field.name.toLowerCase().includes(query)) {
      return { fieldPath: field.name, preview: `${field.name}: ${summaryOf(field.value)}` }
    }
    const match = valueMatch(field.value, query, field.name)
    if (match) return match
  }
  return null
}

function valueMatch(value: FieldValue, query: string, path: string): { fieldPath: string; preview: string } | null {
  const scalar = scalarSearchText(value)
  if (scalar?.toLowerCase().includes(query)) {
    return { fieldPath: path, preview: `${path}: ${summaryOf(value)}` }
  }
  if (value.kind === 'object') {
    if (value.value.actual_type.toLowerCase().includes(query)) {
      return { fieldPath: path, preview: `${path}: ${value.value.actual_type}` }
    }
    for (const [name, child] of Object.entries(value.value.fields)) {
      const childPath = `${path}.${name}`
      if (name.toLowerCase().includes(query)) {
        return { fieldPath: childPath, preview: `${childPath}: ${summaryOf(child)}` }
      }
      const match = valueMatch(child, query, childPath)
      if (match) return match
    }
  } else if (value.kind === 'array') {
    for (let index = 0; index < value.value.length; index += 1) {
      const match = valueMatch(value.value[index], query, `${path}[${index}]`)
      if (match) return match
    }
  } else if (value.kind === 'dict') {
    for (const [key, child] of value.value) {
      const keyText = key.kind === 'enum'
        ? key.value.variant ?? String(key.value.value)
        : String(key.value)
      const childPath = `${path}[${keyText}]`
      if (keyText.toLowerCase().includes(query)) {
        return { fieldPath: childPath, preview: `${childPath}: ${summaryOf(child)}` }
      }
      const match = valueMatch(child, query, childPath)
      if (match) return match
    }
  }
  return null
}

function scalarSearchText(value: FieldValue): string | null {
  switch (value.kind) {
    case 'option_none': return 'None'
    case 'option_some': return scalarSearchText(value.value)
    case 'result_ok': return scalarSearchText(value.value)
    case 'result_err': return scalarSearchText(value.value)
    case 'bool': return String(value.value)
    case 'int': return String(value.value)
    case 'float': return String(value.value)
    case 'string': return value.value
    case 'enum': return `${value.value.enum_name} ${value.value.variant ?? ''} ${value.value.value}`
    case 'ref': return value.value
    default: return null
  }
}
