import { useEffect, useMemo, useState } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import type { ProjectDiff } from '../bindings/ProjectDiff'
import type { ProjectDiffChange } from '../bindings/ProjectDiffChange'
import type { ProjectFileDiff } from '../bindings/ProjectFileDiff'
import type { ProjectRecordDiff } from '../bindings/ProjectRecordDiff'
import type { ProjectRecordSnapshot } from '../bindings/ProjectRecordSnapshot'
import type { RecordColumn } from '../bindings/RecordColumn'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { FileTreeNode } from '../bindings/FileTreeNode'
import type { DimensionInfo } from '../bindings/DimensionInfo'
import { buildFileTreeGroups } from './FileTree'
import * as api from '../api'
import { decodeSemanticTokens } from '../code/lspAdapter'
import { errorMessage } from '../wire'
import { summaryOf } from '../value/fieldValue'
import { CfdCodeEditor, type CodeLineDecoration, type CodeSemanticToken } from './CfdCodeEditor'
import { Icon } from './Icon'
import { RecordView } from './RecordView'
import { TableView, type TableRowPresentation } from './TableView'
import './GitDiffMode.css'

export type GitDiffView = 'record' | 'table' | 'source'

export interface GitDiffSelection {
  filePath: string | null
  coordinate: RecordCoordinate | null
}

interface Props {
  diff: ProjectDiff | null
  loading: boolean
  error: string | null
  selection: GitDiffSelection
  onSelectionChange(selection: GitDiffSelection): void
  onRefresh(): void
}

interface ProjectedTable {
  data: FileRecords
  presentations: ReadonlyMap<RecordRow, TableRowPresentation>
}

const READ_ONLY_CAPABILITIES = {
  can_edit_field: false,
  can_edit_key: false,
  can_insert_record: false,
  can_delete_record: false,
  can_reorder_records: false,
  requires_full_refresh_after_write: false,
}

export function GitDiffSidebar({ diff, loading, error, selection, onSelectionChange, onRefresh, nodes, dimensions }: Props & { nodes: FileTreeNode[], dimensions: DimensionInfo[] }) {
  const groups = useMemo(() => buildFileTreeGroups(diff ? buildDiffTree(diff, nodes) : [], dimensions).filter(group => group.nodes.length > 0), [diff, nodes, dimensions])
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set())
  const toggle = (path: string) => setCollapsed(previous => {
    const next = new Set(previous)
    if (next.has(path)) next.delete(path)
    else next.add(path)
    return next
  })
  const counts = useMemo(() => diff ? changeCounts(diff.records) : null, [diff])
  const renderNode = (node: FileTreeNode, depth: number): React.ReactNode => {
    const records = diff?.records.filter(record => recordPath(record) === node.path) ?? []
    const expanded = !collapsed.has(node.path)
    const expandable = node.is_dir || records.length > 0
    const file = diff?.files.find(item => item.path === node.path)
    return <div key={node.path} role="group">
      <div className={`tree-file${selection.filePath === node.path && !selection.coordinate ? ' selected' : ''}`} style={{ paddingLeft: depth * 12 + 8 }}>
        {expandable ? <button className="btn btn-icon" aria-label={`${expanded ? '折叠' : '展开'} ${node.name}`} aria-expanded={expanded} onClick={() => toggle(node.path)}><Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} /></button> : <span className="git-tree-spacer" />}
        <button className="git-tree-label" title={node.path} onClick={() => node.is_dir ? toggle(node.path) : onSelectionChange({ filePath: node.path, coordinate: null })}>
          <Icon name={node.is_dir ? 'folder' : node.name.endsWith('.cfd') ? 'file-cfd' : 'file'} size={16} className={node.is_dir ? 'icon-folder' : 'icon-file'} />
          <span className="tree-item-label">{node.name}</span>
        </button>
        {!node.is_dir && <ChangeMark change={file?.change ?? aggregateChange(records)} />}
      </div>
      {expanded && node.children.map(child => renderNode(child, depth + 1))}
      {expanded && records.map(record => <button key={coordinateKey(record.coordinate)} className={`git-tree-record tree-file${sameCoordinate(selection.coordinate, record.coordinate) ? ' selected' : ''}`} style={{ paddingLeft: (depth + 2) * 12 + 20 }} onClick={() => onSelectionChange({ filePath: node.path, coordinate: record.coordinate })}>
        <Icon name="record" size={13} /><span className="tree-item-label" title={`${record.coordinate.actual_type}.${record.coordinate.key}`}>{record.coordinate.actual_type}.{record.coordinate.key}</span><ChangeMark change={record.change} />
      </button>)}
    </div>
  }
  return (
    <div className="git-changes-sidebar">
      <div className="sidebar-header git-changes-header">
        <span>Git 变更</span>
        <button className="btn btn-icon" onClick={onRefresh} disabled={loading} title="刷新 Git Diff" aria-label="刷新 Git Diff">
          <Icon name="refresh" size={14} aria-hidden />
        </button>
      </div>
      {counts && (
        <div className="git-changes-counts" aria-label="变更统计">
          <span className="added">+{counts.added}</span>
          <span className="deleted">-{counts.deleted}</span>
          <span className="modified">M{counts.modified}</span>
        </div>
      )}
      <div className="git-changes-list">
        {!diff && !error && <div className="git-diff-message">{loading ? '正在比较 HEAD...' : '尚未获得比较结果'}</div>}
        {error && <div className="git-diff-message error">{error}</div>}
        {diff && !hasProjectDiffChanges(diff) && <div className="git-diff-message">工作区与 HEAD 一致</div>}
        {groups.map(group => <div className="tree-section" key={group.key}>
          <button className="tree-heading" aria-expanded={!collapsed.has(group.key)} onClick={() => toggle(group.key)}>
            <Icon name={collapsed.has(group.key) ? 'chevron-right' : 'chevron-down'} size={12} className="tree-heading-chev" />
            <Icon name={group.icon} size={17} className={`tree-heading-icon ${group.icon}`} /><strong>{group.label}</strong>
          </button>
          {!collapsed.has(group.key) && <div className="tree-content">{group.nodes.map(node => renderNode(node, 0))}</div>}
        </div>)}
      </div>
    </div>
  )
}

export function GitDiffMode({ diff, loading, error, selection, onRefresh, sessionId }: Props & { sessionId: number }) {
  const [view, setView] = useState<GitDiffView>('record')
  useEffect(() => {
    if (selection.coordinate) setView('record')
    else if (selection.filePath) setView('source')
  }, [selection.coordinate, selection.filePath])

  if (error) return <ModeMessage tone="error" text={error} onRefresh={onRefresh} />
  if (!diff) return <ModeMessage text={loading ? '正在比较 HEAD...' : '尚未获得比较结果'} onRefresh={onRefresh} />
  if (!hasProjectDiffChanges(diff)) return <ModeMessage text="工作区与 HEAD 一致" onRefresh={onRefresh} />

  const activeRecord = selectedRecord(diff, selection)
  const activeFile = selectedFile(diff, selection, activeRecord)
  const semanticViews = diff.semantic_available && activeRecord !== null
  const activeView = semanticViews ? view : 'source'
  return (
    <section className="git-diff-mode">
      <header className="git-diff-mode-toolbar">
        <span className="git-diff-head">HEAD {diff.head_oid.slice(0, 12)}</span>
        <div className="document-view-tabs git-diff-view-tabs" role="tablist" aria-label="Git Diff 视图">
          {semanticViews && <DiffTab view="record" active={activeView} onSelect={setView} icon="record" label="记录" />}
          {semanticViews && <DiffTab view="table" active={activeView} onSelect={setView} icon="table" label="表格" />}
          <DiffTab view="source" active={activeView} onSelect={setView} icon="code" label="源码" />
        </div>
        {loading && <span className="git-diff-updating">正在更新...</span>}
        <button className="btn btn-icon" onClick={onRefresh} disabled={loading} title="刷新 Git Diff" aria-label="刷新 Git Diff">
          <Icon name="refresh" size={14} aria-hidden />
        </button>
      </header>
      {activeView === 'record' ? (
        <RecordComparison record={activeRecord} />
      ) : activeView === 'table' ? (
        <TableComparison diff={diff} activeRecord={activeRecord} filePath={activeFile?.path ?? selection.filePath} />
      ) : (
        <SourceComparison file={activeFile} sessionId={sessionId} />
      )}
    </section>
  )
}

function DiffTab({ view, active, onSelect, icon, label, disabled = false }: {
  view: GitDiffView
  active: GitDiffView
  onSelect(view: GitDiffView): void
  icon: 'record' | 'table' | 'code'
  label: string
  disabled?: boolean
}) {
  return (
    <button className={`tab-btn tab-view${active === view ? ' active' : ''}`} role="tab" aria-selected={active === view} disabled={disabled} onClick={() => onSelect(view)}>
      <Icon name={icon} size={13} aria-hidden />
      {label}
    </button>
  )
}

function RecordComparison({ record }: { record: ProjectRecordDiff | null }) {
  if (!record) return <div className="git-diff-message centered">没有可显示的记录变化</div>
  const changedPaths = new Set(record.fields.map(field => field.path))
  return (
    <div className={`git-record-comparison ${record.change}`}>
      <RecordPane label="HEAD" snapshot={record.before} record={record} changedPaths={changedPaths} />
      <RecordPane label="当前工作区" snapshot={record.after} record={record} changedPaths={changedPaths} />
    </div>
  )
}

function RecordPane({ label, snapshot, record, changedPaths }: {
  label: string
  snapshot?: ProjectRecordSnapshot
  record: ProjectRecordDiff
  changedPaths: ReadonlySet<string>
}) {
  const side = label === 'HEAD' ? 'before' : 'after'
  if (!snapshot) {
    return <section className={`git-record-pane missing ${record.change}`}><header>{label}</header><div>{side === 'before' ? 'HEAD 中不存在' : '当前工作区中不存在'}</div></section>
  }
  const data = snapshotFileRecords(record, snapshot, 0)
  return (
    <section className={`git-record-pane ${record.change}`}>
      <header>{label}</header>
      <RecordView
        data={data}
        coordinate={record.coordinate}
        typeFilter={record.coordinate.actual_type}
        readOnly
        hideRecordList
        onOpenRecord={() => {}}
        diffChangedPaths={record.change === 'modified' ? changedPaths : undefined}
      />
    </section>
  )
}

function TableComparison({ diff, activeRecord, filePath }: {
  diff: ProjectDiff
  activeRecord: ProjectRecordDiff | null
  filePath: string | null
}) {
  const [changedOnly, setChangedOnly] = useState(false)
  const actualType = activeRecord?.coordinate.actual_type
    ?? diff.records.find(record => !filePath || recordPath(record) === filePath)?.coordinate.actual_type
  const projected = useMemo(
    () => projectTable(diff, filePath, actualType ?? ''),
    [actualType, diff, filePath],
  )
  if (!actualType || projected.data.records.length === 0) {
    return <div className="git-diff-message centered">没有可显示的记录变化</div>
  }
  return (
    <div className="git-table-comparison">
      <label className="git-table-filter"><input type="checkbox" checked={changedOnly} onChange={event => setChangedOnly(event.target.checked)} />仅修改列</label>
    <TableView
      data={projected.data}
      activeType={actualType}
      readOnly
      visibleColumns={changedOnly ? changedTableColumns(projected) : undefined}
      onOpenRecord={() => {}}
      rowPresentation={row => projected.presentations.get(row)}
    />
    </div>
  )
}

function SourceComparison({ file, sessionId }: { file: ProjectFileDiff | null, sessionId: number }) {
  const decorations = useMemo(() => file ? sourceLineDecorations(file.patch) : { before: [], after: [] }, [file])
  if (!file) return <div className="git-diff-message centered">没有可显示的源码变化</div>
  return (
    <div className={`git-source-comparison ${file.change}`}>
      <SourcePane sessionId={sessionId} filePath={file.path} label="HEAD" source={file.before} missing="HEAD 中不存在" decorations={decorations.before} />
      <SourcePane sessionId={sessionId} filePath={file.path} label="当前工作区" source={file.after} missing="当前工作区中不存在" decorations={decorations.after} />
    </div>
  )
}

function SourcePane({ label, source, missing, decorations, sessionId, filePath }: {
  sessionId: number
  filePath: string
  label: string
  source?: string
  missing: string
  decorations: readonly CodeLineDecoration[]
}) {
  const [highlight, setHighlight] = useState<{ source: string, filePath: string, tokens: CodeSemanticToken[] } | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    setHighlight(null)
    setError(null)
    if (source !== undefined && /\.(cft|cfd)$/i.test(filePath) && api.isTauri) {
      api.highlightSourceSnapshot(sessionId, filePath, source).then(state => {
        if (alive) setHighlight({ source, filePath, tokens: decodeSemanticTokens(source, state) })
      }).catch(cause => { if (alive) setError(errorMessage(cause)) })
    }
    return () => { alive = false }
  }, [sessionId, filePath, source])
  return (
    <section className="git-source-pane">
      <header>{label}{error && <span className="error" title={error}>高亮加载失败</span>}</header>
      {source === undefined
        ? <div className="git-source-missing">{missing}</div>
        : <CfdCodeEditor value={source} onChange={() => {}} readOnly semanticTokens={highlight?.source === source && highlight.filePath === filePath ? highlight.tokens : []} lineDecorations={decorations} />}
    </section>
  )
}

function ModeMessage({ text, tone = '', onRefresh }: { text: string, tone?: string, onRefresh(): void }) {
  return (
    <div className={`git-diff-message centered ${tone}`}>
      <span>{text}</span>
      <button className="btn btn-outlined" onClick={onRefresh}><Icon name="refresh" size={13} />刷新</button>
    </div>
  )
}

function ChangeMark({ change }: { change: ProjectDiffChange }) {
  return <span className={`git-change-mark ${change}`}>{change === 'added' ? '+' : change === 'deleted' ? '-' : 'M'}</span>
}

export function sourceLineDecorations(patch: string): { before: CodeLineDecoration[], after: CodeLineDecoration[] } {
  const before: CodeLineDecoration[] = []
  const after: CodeLineDecoration[] = []
  const lines = patch.split('\n')
  let beforeLine = 0
  let afterLine = 0
  for (let index = 0; index < lines.length;) {
    const header = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(lines[index] ?? '')
    if (header) {
      beforeLine = Number(header[1])
      afterLine = Number(header[2])
      index += 1
      continue
    }
    const line = lines[index] ?? ''
    if (line.startsWith(' ')) {
      beforeLine += 1
      afterLine += 1
      index += 1
      continue
    }
    if (line.startsWith('-') || line.startsWith('+')) {
      const removed: number[] = []
      const added: number[] = []
      while (index < lines.length && (lines[index]?.startsWith('-') || lines[index]?.startsWith('+'))) {
        if (lines[index]?.startsWith('-')) removed.push(beforeLine++)
        else added.push(afterLine++)
        index += 1
      }
      const modified = removed.length > 0 && added.length > 0
      before.push(...removed.map(lineNumber => ({ line: lineNumber, className: modified ? 'cm-diff-modified' : 'cm-diff-deleted' })))
      after.push(...added.map(lineNumber => ({ line: lineNumber, className: modified ? 'cm-diff-modified' : 'cm-diff-added' })))
      continue
    }
    index += 1
  }
  return { before, after }
}

export function projectTable(diff: ProjectDiff, filePath: string | null, actualType: string): ProjectedTable {
  const rows: RecordRow[] = []
  const presentations = new Map<RecordRow, TableRowPresentation>()
  for (const record of diff.records) {
    if (record.coordinate.actual_type !== actualType) continue
    if (filePath && recordPath(record) !== filePath) continue
    const changedFields = record.change === 'modified'
      ? new Set(record.fields.map(field => topLevelPath(field.path)))
      : new Set<string>()
    const add = (snapshot: ProjectRecordSnapshot | undefined, version: 'HEAD' | '当前', side: string) => {
      if (!snapshot) return
      const row = snapshotRow(record, snapshot, rows.length, 0)
      rows.push(row)
      presentations.set(row, {
        id: `${coordinateKey(record.coordinate)}:${side}`,
        version,
        change: record.change,
        changedFields,
      })
    }
    add(record.before, 'HEAD', 'before')
    add(record.after, '当前', 'after')
  }
  const file = filePath ?? rows[0]?.display_path ?? ''
  const columns = columnsFor(rows, actualType)
  const sizedRows = rows.map((row, index) => ({ ...row, container_index: index, container_size: rows.length }))
  // 行元数据使用对象身份关联，替换容器位置时同步重建索引。
  const sizedPresentations = new Map<RecordRow, TableRowPresentation>()
  rows.forEach((row, index) => sizedPresentations.set(sizedRows[index], presentations.get(row)!))
  return {
    data: { revision: diff.target_revision, file_path: file, type_names: actualType ? [actualType] : [], columns, records: sizedRows, capabilities: READ_ONLY_CAPABILITIES },
    presentations: sizedPresentations,
  }
}

export function hasProjectDiffChanges(diff: ProjectDiff): boolean {
  return diff.files.length > 0 || diff.records.length > 0
}

function snapshotFileRecords(record: ProjectRecordDiff, snapshot: ProjectRecordSnapshot, revision: number): FileRecords {
  const row = snapshotRow(record, snapshot, 0, 1)
  return {
    revision,
    file_path: snapshot.file_path,
    type_names: [record.coordinate.actual_type],
    columns: columnsFor([row], record.coordinate.actual_type),
    records: [row],
    capabilities: READ_ONLY_CAPABILITIES,
  }
}

function snapshotRow(record: ProjectRecordDiff, snapshot: ProjectRecordSnapshot, index: number, size: number): RecordRow {
  const fields = snapshot.values.map(item => ({ name: item.path, value: item.value, missing: false, annotation: null }))
  return {
    coordinate: { ...record.coordinate },
    display_path: snapshot.file_path,
    container_index: index,
    container_size: size,
    fields,
    field_index: Object.fromEntries(fields.map((field, fieldIndex) => [field.name, fieldIndex])),
    field_summaries: Object.fromEntries(fields.map(field => [field.name, summaryOf(field.value)])),
    field_diagnostics: [],
    diagnostic_severity: null,
  }
}

function columnsFor(rows: readonly RecordRow[], actualType: string): RecordColumn[] {
  const names = new Set(rows.flatMap(row => row.fields.map(field => field.name)))
  return [...names].map(name => ({
    name,
    type_names: [actualType],
    max_summary_len: Math.max(0, ...rows.map(row => row.field_summaries[name]?.length ?? 0)),
  }))
}

function selectedRecord(diff: ProjectDiff, selection: GitDiffSelection): ProjectRecordDiff | null {
  if (selection.coordinate) {
    const selected = diff.records.find(record => sameCoordinate(selection.coordinate, record.coordinate))
    if (selected) return selected
  }
  return diff.records.find(record => !selection.filePath || recordPath(record) === selection.filePath)
    ?? null
}

function selectedFile(diff: ProjectDiff, selection: GitDiffSelection, record: ProjectRecordDiff | null): ProjectFileDiff | null {
  const path = selection.filePath ?? (record ? recordPath(record) : null)
  return (path ? diff.files.find(file => file.path === path) : diff.files[0]) ?? null
}

export function changedTableColumns(projected: ProjectedTable): string[] {
  // 新增和删除记录的全部字段都属于变化；嵌套字段变化投影到顶层列。
  const names = new Set<string>()
  for (const row of projected.data.records) {
    const presentation = projected.presentations.get(row)!
    for (const name of presentation.change === 'modified' ? presentation.changedFields : row.fields.map(field => field.name)) names.add(name)
  }
  return projected.data.columns.filter(column => names.has(column.name)).map(column => column.name)
}

export function buildDiffTree(diff: ProjectDiff, nodes: FileTreeNode[]): FileTreeNode[] {
  const paths = new Set(changedPaths(diff))
  const clone = (items: FileTreeNode[]): FileTreeNode[] => items.map(node => ({ ...node, children: clone(node.children) }))
  const tree = clone(nodes)
  const all = (items: FileTreeNode[]): FileTreeNode[] => items.flatMap(node => [node, ...all(node.children)])
  // 当前树提供输入分组信息，HEAD 独有路径补回同一目录层级。
  for (const path of paths) {
    if (all(tree).some(node => node.path === path)) continue
    let parent = all(tree).filter(node => node.is_dir && path.startsWith(`${node.path}/`)).sort((a, b) => b.path.length - a.path.length)[0]
    const parts = (parent ? path.slice(parent.path.length + 1) : path).split('/')
    let siblings = parent?.children ?? tree
    let prefix = parent?.path ?? ''
    parts.forEach((name, index) => {
      prefix = prefix ? `${prefix}/${name}` : name
      const isDir = index < parts.length - 1
      let node = siblings.find(item => item.path === prefix)
      if (!node) {
        node = { name, path: prefix, is_dir: isDir, in_sources: !isDir, in_schema: path.endsWith('.cft'), in_data: !path.endsWith('.cft'), first_source_descendant: isDir ? path : null, children: [] }
        siblings.push(node)
      }
      parent = node
      siblings = node.children
    })
  }
  const prune = (items: FileTreeNode[]): FileTreeNode[] => items.flatMap(node => {
    const children = prune(node.children)
    return node.is_dir ? children.length ? [{ ...node, children }] : [] : paths.has(node.path) ? [{ ...node, in_sources: true }] : []
  }).sort((a, b) => Number(b.is_dir) - Number(a.is_dir) || a.name.localeCompare(b.name))
  return prune(tree)
}

function changedPaths(diff: ProjectDiff): string[] {
  return [...new Set([
    ...diff.files.map(file => file.path),
    ...diff.records.map(recordPath),
  ])]
}

function recordPath(record: ProjectRecordDiff): string {
  return record.after?.file_path ?? record.before?.file_path ?? ''
}

function coordinateKey(coordinate: RecordCoordinate): string {
  return `${coordinate.actual_type}\u0000${coordinate.key}`
}

function sameCoordinate(left: RecordCoordinate | null, right: RecordCoordinate): boolean {
  return !!left && left.actual_type === right.actual_type && left.key === right.key
}

function aggregateChange(records: readonly ProjectRecordDiff[]): ProjectDiffChange {
  if (records.some(record => record.change === 'modified')) return 'modified'
  if (records.some(record => record.change === 'added')) return 'added'
  return 'deleted'
}

function changeCounts(records: readonly ProjectRecordDiff[]) {
  return records.reduce((counts, record) => {
    counts[record.change] += 1
    return counts
  }, { added: 0, deleted: 0, modified: 0 })
}

function topLevelPath(path: string): string {
  return path.split(/[.[]/, 1)[0] ?? path
}
