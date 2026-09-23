import { useEffect, useMemo, useState } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import type { FileTypeOption } from '../bindings/FileTypeOption'
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
import { coordinateId, sameCoordinate, type FieldPathSegment } from '../wire'
import type { EditorSelection } from '../state/editorSelection'
import { typeColor } from '../utils/typeColor'
import { CfdCodeEditor, type CodeLineDecoration, type CodeSemanticToken } from './CfdCodeEditor'
import { Icon } from './Icon'
import { RecordView } from './RecordView'
import { TableView, type TableRowPresentation } from './TableView'
import './GitDiffMode.css'

export type GitDiffView = 'record' | 'table' | 'source'

export interface GitDiffSelection {
  filePath: string | null
  typeName?: string | null
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

// 文件级 diff 索引，一次遍历建成，避免侧栏与主视图反复全量扫描。
export interface DiffIndex {
  fileByPath: ReadonlyMap<string, ProjectFileDiff>
  recordsByFile: ReadonlyMap<string, ProjectRecordDiff[]>
  changedPaths: ReadonlySet<string>
}

export function buildDiffIndex(diff: ProjectDiff | null): DiffIndex {
  const fileByPath = new Map<string, ProjectFileDiff>()
  const recordsByFile = new Map<string, ProjectRecordDiff[]>()
  const changedPaths = new Set<string>()
  if (!diff) return { fileByPath, recordsByFile, changedPaths }
  for (const file of diff.files) {
    fileByPath.set(file.path, file)
    changedPaths.add(file.path)
  }
  for (const record of diff.records) {
    const path = recordPath(record)
    if (!path) continue
    changedPaths.add(path)
    const list = recordsByFile.get(path)
    if (list) list.push(record)
    else recordsByFile.set(path, [record])
  }
  return { fileByPath, recordsByFile, changedPaths }
}

export function GitDiffSidebar({ diff, loading, error, selection, onSelectionChange, onRefresh, nodes, dimensions, fileTypes }: Props & { nodes: FileTreeNode[], dimensions: DimensionInfo[], fileTypes: Record<string, FileTypeOption[] | undefined> }) {
  // 左侧文件树与主界面共用分组/图标/排序，仅保留变化文件及其祖先目录，不显示记录节点。
  const index = useMemo(() => buildDiffIndex(diff), [diff])
  const filtered = useMemo(() => buildDiffTree(diff, nodes, index.changedPaths), [diff, nodes, index])
  const groups = useMemo(() => buildFileTreeGroups(filtered, []).filter(group => group.nodes.length > 0), [filtered, dimensions])
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set())
  const toggle = (path: string) => setCollapsed(previous => {
    const next = new Set(previous)
    if (next.has(path)) next.delete(path)
    else next.add(path)
    return next
  })
  const counts = useMemo(() => diff ? changeCounts(diff.records) : null, [diff])
  // 选中文件时展开祖先目录，保证定位可见。
  useEffect(() => {
    if (!selection.filePath) return
    setCollapsed(previous => {
      const next = new Set(previous)
      let changed = false
      const parts = selection.filePath!.split('/')
      let prefix = ''
      for (let i = 0; i < parts.length - 1; i += 1) {
        prefix = prefix ? `${prefix}/${parts[i]}` : parts[i]!
        if (next.delete(prefix)) changed = true
      }
      return changed ? next : previous
    })
  }, [selection.filePath])

  const selectFile = (path: string, typeName: string) => {
    onSelectionChange({ filePath: path, typeName: typeName || null, coordinate: null })
  }

  const renderNode = (node: FileTreeNode, depth: number): React.ReactNode => {
    if (node.is_dir) {
      const expanded = !collapsed.has(node.path)
      return (
        <div key={node.path} role="group">
          <div className="tree-dir-label" style={{ paddingLeft: depth * 12 + 8 }} role="treeitem" aria-level={depth + 1} aria-expanded={expanded} tabIndex={0} data-path={node.path}
            onClick={() => toggle(node.path)}
            onKeyDown={event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); event.stopPropagation(); toggle(node.path) } }}>
            <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} className="tree-dir-chevron" aria-hidden />
            <Icon name="folder" size={16} className="icon-folder" aria-hidden />
            <span>{node.name}</span>
          </div>
          {!expanded ? null : node.children.map(child => renderNode(child, depth + 1))}
        </div>
      )
    }
    const types = fileTypes[node.path] ?? []
    const file = index.fileByPath.get(node.path)
    const records = index.recordsByFile.get(node.path) ?? []
    const mark = file?.change ?? (records.length > 0 ? aggregateChange(records) : null)
    const isCfd = node.name.endsWith('.cfd')
    // 多类型文件展开类型子行，与主文件树一致；单击类型即切换视图类型。
    // 无任何类型命中变化记录时（如本地化生成文件、纯源码文本变更），或仅一个类型有变化时，
    // 退化为单行可选中文件，保证变化文件始终可点击加载，且单类型无需展开下拉。
    const changedTypes = !node.is_dir && types.length > 1
      ? types.filter(type => records.some(record => record.coordinate.actual_type === type.name))
      : []
    const showTypeChildren = changedTypes.length > 1
    if (!node.is_dir && types.length > 1 && showTypeChildren) {
      const expanded = !collapsed.has(node.path)
      const containsSelection = selection.filePath === node.path
      return (
        <div key={node.path} role="group">
          <div className={`tree-file tree-file-parent${containsSelection ? ' contains-selection' : ''}${isCfd ? ' is-cfd' : ''}`}
            style={{ paddingLeft: (depth + 1) * 12 + 8 }} role="treeitem" aria-level={depth + 1} aria-expanded={expanded}
            tabIndex={0} data-path={node.path} data-file-path={node.path} title={node.path} onClick={() => toggle(node.path)}>
            <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} className="tree-file-chevron" aria-hidden />
            <Icon name={isCfd ? 'file-cfd' : 'file'} size={16} className="icon-file" aria-hidden />
            <span className="tree-item-label">{node.name}</span>
            {mark && <ChangeMark change={mark} />}
          </div>
          {!expanded ? null : changedTypes.map(type => {
            const selected = selection.filePath === node.path && (selection.typeName ?? types[0]?.name) === type.name
            const typeRecords = records.filter(record => record.coordinate.actual_type === type.name)
            return (
              <div key={type.name}
                className={`tree-type${selected ? ' selected' : ''}`}
                style={{ paddingLeft: (depth + 2) * 12 + 20, '--type-color': typeColor(type.name) } as React.CSSProperties}
                role="treeitem" aria-level={depth + 2} aria-selected={selected} tabIndex={0}
                data-path={`${node.path}  ${type.name}`} data-file-path={node.path} data-type-name={type.name}
                onClick={() => selectFile(node.path, type.name)}
                title={type.display_name === type.name ? type.name : `${type.display_name} (${type.name})`}>
                <span className="tree-type-dot" aria-hidden />
                <span className="tree-item-label">{type.display_name}</span>
                <span className="tree-type-count">{typeRecords.length}</span>
                <ChangeMark change={aggregateChange(typeRecords)} />
              </div>
            )
          })}
        </div>
      )
    }
    const selected = selection.filePath === node.path
    // 多类型文件中仅一个类型有变化时，直接选中该类型，省去展开下拉。
    const typeName = changedTypes.length === 1 ? changedTypes[0]!.name : types[0]?.name ?? ''
    return (
      <div key={node.path}
        className={`tree-file${selected ? ' selected' : ''}${isCfd ? ' is-cfd' : ''}`}
        style={{ paddingLeft: (depth + 1) * 12 + 8 }}
        role="treeitem" aria-level={depth + 1} aria-selected={selected} tabIndex={0}
        data-path={node.path} data-file-path={node.path} data-type-name={typeName}
        onClick={() => selectFile(node.path, typeName)}
        onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); selectFile(node.path, typeName) } }}
        title={node.path}>
        <span className="tree-file-chevron-spacer" aria-hidden />
        <Icon name={isCfd ? 'file-cfd' : 'file'} size={16} className="icon-file" aria-hidden />
        <span className="tree-item-label">{node.name}</span>
        {mark && <ChangeMark change={mark} />}
      </div>
    )
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

export function GitDiffMode({ diff, loading, error, selection, onSelectionChange, onRefresh, sessionId, fileTypes }: Props & { sessionId: number, fileTypes: Record<string, FileTypeOption[] | undefined> }) {
  const index = useMemo(() => buildDiffIndex(diff), [diff])
  const fallbackFile = diff?.files[0]?.path ?? firstChangedRecordPath(diff) ?? null
  const activeFilePath = selection.filePath ?? fallbackFile
  const typeOptions = (activeFilePath ? fileTypes[activeFilePath] : undefined) ?? []
  const fallbackType = typeOptions[0]?.name
    ?? (activeFilePath ? (index.recordsByFile.get(activeFilePath) ?? [])[0]?.coordinate.actual_type : undefined)
    ?? null
  const activeTypeName = selection.typeName ?? fallbackType
  const isSourceFile = !!activeFilePath && (activeFilePath.endsWith('.cft') || activeFilePath.endsWith('.yaml') || activeFilePath.endsWith('.yml'))
  const recordsForView = useMemo(() => {
    if (!activeFilePath) return []
    const list = index.recordsByFile.get(activeFilePath) ?? []
    if (!activeTypeName) return list
    return list.filter(record => record.coordinate.actual_type === activeTypeName)
  }, [index, activeFilePath, activeTypeName])
  const isSingleton = useMemo(() => {
    if (!activeFilePath || !activeTypeName) return false
    return typeOptions.find(option => option.name === activeTypeName)?.is_singleton ?? false
  }, [typeOptions, activeFilePath, activeTypeName])
  // 视图固定——源码文件/语义不可用时仅源码；单例固定记录+源码；普通记录固定记录+表格+源码。
  const availableViews: GitDiffView[] = useMemo(() => {
    if (!diff?.semantic_available || !activeFilePath) return ['source']
    if (isSourceFile) return ['source']
    if (recordsForView.length === 0) return ['source']
    return isSingleton ? ['record', 'source'] : ['record', 'table', 'source']
  }, [diff?.semantic_available, activeFilePath, isSourceFile, recordsForView.length, isSingleton])

  const [view, setView] = useState<GitDiffView>('record')
  // 同步钳制当前视图，保证首屏/SSR 即为可用视图；文件/类型切换时回到首个可用视图。
  const clampedView = availableViews.includes(view) ? view : (availableViews[0] ?? 'source')
  useEffect(() => {
    if (clampedView !== view) setView(clampedView)
  }, [clampedView, view])
  // 记录/表格视图共用的字段过滤开关，默认仅显示修改字段。
  const [changedOnly, setChangedOnly] = useState(true)

  if (error) return <ModeMessage tone="error" text={error} onRefresh={onRefresh} />
  if (!diff) return <ModeMessage text={loading ? '正在比较 HEAD...' : '尚未获得比较结果'} onRefresh={onRefresh} />
  if (!hasProjectDiffChanges(diff)) return <ModeMessage text="工作区与 HEAD 一致" onRefresh={onRefresh} />
  if (!activeFilePath) return <ModeMessage text="没有可显示的变化文件" onRefresh={onRefresh} />

  const activeFile = index.fileByPath.get(activeFilePath) ?? diff.files.find(file => file.path === activeFilePath) ?? null
  return (
    <section className="git-diff-mode git-diff-selectable">
      <header className="git-diff-mode-toolbar">
        <span className="git-diff-head">HEAD {diff.head_oid.slice(0, 12)}</span>
        <div className="document-view-tabs git-diff-view-tabs" role="tablist" aria-label="Git Diff 视图">
          {availableViews.includes('record') && <DiffTab view="record" active={clampedView} onSelect={setView} icon="record" label="记录" />}
          {availableViews.includes('table') && <DiffTab view="table" active={clampedView} onSelect={setView} icon="table" label="表格" />}
          {availableViews.includes('source') && <DiffTab view="source" active={clampedView} onSelect={setView} icon="code" label="源码" />}
        </div>
        {(clampedView === 'record' || clampedView === 'table') && (
          <label className="git-diff-changed-only"><input type="checkbox" checked={changedOnly} onChange={event => setChangedOnly(event.target.checked)} />仅修改字段</label>
        )}
        {loading && <span className="git-diff-updating">正在更新...</span>}
        <button className="btn btn-icon" onClick={onRefresh} disabled={loading} title="刷新 Git Diff" aria-label="刷新 Git Diff">
          <Icon name="refresh" size={14} aria-hidden />
        </button>
      </header>
      {clampedView === 'record' ? (
        <RecordDiffView records={recordsForView} selection={selection} onSelectionChange={onSelectionChange} changedOnly={changedOnly} />
      ) : clampedView === 'table' ? (
        <TableComparison diff={diff} filePath={activeFilePath} actualType={activeTypeName ?? ''} changedOnly={changedOnly} />
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

// 记录视图左侧只列变化记录（支持搜索定位），右侧并排显示选中记录的 HEAD/当前对比。
function RecordDiffView({ records, selection, onSelectionChange, changedOnly }: {
  records: ProjectRecordDiff[]
  selection: GitDiffSelection
  onSelectionChange(selection: GitDiffSelection): void
  changedOnly: boolean
}) {
  const [search, setSearch] = useState('')
  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase()
    if (!query) return records
    return records.filter(record => `${record.coordinate.actual_type}.${record.coordinate.key}`.toLowerCase().includes(query))
  }, [records, search])
  const activeRecord = useMemo(() => {
    if (selection.coordinate) {
      const hit = records.find(record => sameCoordinate(record.coordinate, selection.coordinate!))
      if (hit) return hit
    }
    return filtered[0] ?? records[0] ?? null
  }, [records, filtered, selection.coordinate])
  useEffect(() => {
    // 文件切换后自动选中首个变化记录，保持右侧对比有内容。
    if (activeRecord && !selection.coordinate) {
      onSelectionChange({ ...selection, coordinate: activeRecord.coordinate })
    }
  }, [activeRecord, selection, onSelectionChange])
  if (records.length === 0) return <div className="git-diff-message centered">没有可显示的记录变化</div>
  const changedPaths = useMemo(() => new Set(activeRecord?.fields.map(field => field.path) ?? []), [activeRecord])
  // 仅修改字段模式下，记录窗格只保留变化顶层字段，并自动展开其祖先路径。
  const visibleTopFields = useMemo(() => {
    if (!changedOnly || !activeRecord || activeRecord.change !== 'modified') return undefined
    return new Set([...changedPaths].map(topLevelPath))
  }, [changedOnly, activeRecord, changedPaths])
  const autoExpandPaths = useMemo(() => {
    if (!changedOnly || !activeRecord || activeRecord.change !== 'modified') return undefined
    return ancestorPathKeys(changedPaths)
  }, [changedOnly, activeRecord, changedPaths])
  return (
    <div className="git-record-diff">
      <aside className="git-record-list" aria-label="变化记录">
        <div className="git-record-list-search">
          <input type="search" placeholder="搜索变化记录" value={search} onChange={event => setSearch(event.target.value)} aria-label="搜索变化记录" />
        </div>
        <div className="git-record-list-items" role="listbox" aria-label="变化记录列表">
          {filtered.map(record => {
            const selected = !!activeRecord && sameCoordinate(activeRecord.coordinate, record.coordinate)
            return (
              <button key={coordinateId(record.coordinate)} role="option" aria-selected={selected}
                className={`git-record-list-item${selected ? ' selected' : ''}`}
                onClick={() => onSelectionChange({ ...selection, coordinate: record.coordinate })}>
                <span className="tree-item-label" title={`${record.coordinate.actual_type}.${record.coordinate.key}`}>{record.coordinate.actual_type}.{record.coordinate.key}</span>
                <ChangeMark change={record.change} />
              </button>
            )
          })}
          {filtered.length === 0 && <div className="git-diff-message">无匹配记录</div>}
        </div>
      </aside>
      <div className={`git-record-comparison ${activeRecord?.change ?? ''}`}>
        <RecordPane label="HEAD" snapshot={activeRecord?.before} record={activeRecord} changedPaths={changedPaths} visibleTopFields={visibleTopFields} autoExpandPaths={autoExpandPaths} />
        <RecordPane label="当前工作区" snapshot={activeRecord?.after} record={activeRecord} changedPaths={changedPaths} visibleTopFields={visibleTopFields} autoExpandPaths={autoExpandPaths} />
      </div>
    </div>
  )
}

function RecordPane({ label, snapshot, record, changedPaths, visibleTopFields, autoExpandPaths }: {
  label: string
  snapshot?: ProjectRecordSnapshot
  record: ProjectRecordDiff | null
  changedPaths: ReadonlySet<string>
  visibleTopFields?: ReadonlySet<string>
  autoExpandPaths?: ReadonlySet<string>
}) {
  // 只读但保留选中能力， selection 状态本地维护，不触发任何写操作。
  const [selection, setSelection] = useState<EditorSelection | null>(null)
  const side = label === 'HEAD' ? 'before' : 'after'
  if (!record) return <div className="git-diff-message centered">没有可显示的记录变化</div>
  if (!snapshot) {
    return <section className={`git-record-pane missing ${record.change}`}><header>{label}</header><div>{side === 'before' ? 'HEAD 中不存在' : '当前工作区中不存在'}</div></section>
  }
  const data = snapshotFileRecords(record, snapshot, 0, visibleTopFields)
  const handleSelectValue = (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]) => {
    setSelection({ kind: 'value', filePath: snapshot.file_path, coordinate, fieldPath, rangeAnchor: { coordinate, fieldPath } })
  }
  return (
    <section className={`git-record-pane ${record.change}`}>
      <header>{label}</header>
      <RecordView
        key={`${coordinateId(record.coordinate)}:${snapshot.file_path}:${visibleTopFields ? 'changed' : 'all'}`}
        data={data}
        coordinate={record.coordinate}
        typeFilter={record.coordinate.actual_type}
        readOnly
        hideRecordList
        selection={selection}
        onSelectValue={handleSelectValue}
        onSelectRecord={coordinate => setSelection({ kind: 'record', filePath: snapshot.file_path, coordinate, coordinates: [coordinate], anchor: coordinate })}
        onOpenRecord={() => {}}
        diffChangedPaths={record.change === 'modified' ? changedPaths : undefined}
        initialExpandedPaths={autoExpandPaths}
      />
    </section>
  )
}

// 表格 diff——新增/删除单行（整行绿/红），修改显示两行（先原本 HEAD，后修改后当前），变化单元格深黄高亮。
function TableComparison({ diff, filePath, actualType, changedOnly }: {
  diff: ProjectDiff
  filePath: string | null
  actualType: string
  changedOnly: boolean
}) {
  const [selection, setSelection] = useState<EditorSelection | null>(null)
  const projected = useMemo(
    () => projectTable(diff, filePath, actualType),
    [diff, filePath, actualType],
  )
  const visible = useMemo(() => {
    if (!changedOnly) return undefined
    const columns = changedTableColumns(projected)
    return columns.length > 0 ? columns : undefined
  }, [changedOnly, projected])
  if (!actualType || projected.data.records.length === 0) {
    return <div className="git-diff-message centered">没有可显示的记录变化</div>
  }
  return (
    <div className="git-table-comparison">
    <TableView
      data={projected.data}
      activeType={actualType}
      readOnly
      selection={selection}
      onSelectValue={(coordinate, fieldPath) => setSelection({ kind: 'value', filePath: projected.data.file_path, coordinate, fieldPath, rangeAnchor: { coordinate, fieldPath } })}
      onSelectRecord={(coordinate) => setSelection({ kind: 'record', filePath: projected.data.file_path, coordinate, coordinates: [coordinate], anchor: coordinate })}
      onClearSelection={() => setSelection(null)}
      visibleColumns={visible}
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

// 表格投影——新增/删除单行，修改显示两行（先原本 HEAD，后修改后当前），行级 change 决定整行底色。
export function projectTable(diff: ProjectDiff, filePath: string | null, actualType: string): ProjectedTable {
  const rows: RecordRow[] = []
  const presentations = new Map<RecordRow, TableRowPresentation>()
  if (!actualType) {
    return {
      data: { revision: diff.target_revision, file_path: filePath ?? '', type_names: [], columns: [], records: [], capabilities: READ_ONLY_CAPABILITIES },
      presentations,
    }
  }
  for (const record of diff.records) {
    if (record.coordinate.actual_type !== actualType) continue
    if (filePath && recordPath(record) !== filePath) continue
    if (record.change === 'modified' && record.before && record.after) {
      const changedFields = new Set(record.fields.map(field => topLevelPath(field.path)))
      const beforeRow = snapshotRow(record, record.before, rows.length, 0)
      rows.push(beforeRow)
      presentations.set(beforeRow, {
        id: `${coordinateId(record.coordinate)}:before`,
        version: 'HEAD',
        change: record.change,
        changedFields,
        groupStart: true,
      })
      const afterRow = snapshotRow(record, record.after, rows.length, 0)
      rows.push(afterRow)
      presentations.set(afterRow, {
        id: `${coordinateId(record.coordinate)}:after`,
        version: '当前',
        change: record.change,
        changedFields,
      })
      continue
    }
    const snapshot = record.after ?? record.before
    if (!snapshot) continue
    const row = snapshotRow(record, snapshot, rows.length, 0)
    rows.push(row)
    presentations.set(row, {
      id: coordinateId(record.coordinate),
      version: record.after ? '当前' : 'HEAD',
      change: record.change,
      changedFields: new Set<string>(),
      groupStart: true,
    })
  }
  const file = filePath ?? rows[0]?.display_path ?? ''
  const columns = columnsFor(rows, actualType)
  const sizedRows = rows.map((row, index) => ({ ...row, container_index: index, container_size: rows.length }))
  // 行元数据使用对象身份关联，替换容器位置时同步重建索引。
  const sizedPresentations = new Map<RecordRow, TableRowPresentation>()
  rows.forEach((row, index) => sizedPresentations.set(sizedRows[index]!, presentations.get(row)!))
  return {
    data: { revision: diff.target_revision, file_path: file, type_names: actualType ? [actualType] : [], columns, records: sizedRows, capabilities: READ_ONLY_CAPABILITIES },
    presentations: sizedPresentations,
  }
}

export function hasProjectDiffChanges(diff: ProjectDiff): boolean {
  return diff.files.length > 0 || diff.records.length > 0
}

function snapshotFileRecords(record: ProjectRecordDiff, snapshot: ProjectRecordSnapshot, revision: number, visibleTopFields?: ReadonlySet<string>): FileRecords {
  // 仅修改字段模式下，只保留变化顶层字段；新增/删除记录始终显示全部字段。
  const values = visibleTopFields
    ? snapshot.values.filter(item => visibleTopFields.has(topLevelPath(item.path)))
    : snapshot.values
  const row = snapshotRow(record, { ...snapshot, values }, 0, 1)
  return {
    revision,
    file_path: snapshot.file_path,
    type_names: [record.coordinate.actual_type],
    columns: columnsFor([row], record.coordinate.actual_type),
    records: [row],
    capabilities: READ_ONLY_CAPABILITIES,
  }
}

// 由变化字段路径展开全部祖先路径键（如 a.b[0].c → a、a.b、a.b[0]），供记录视图预展开。
export function ancestorPathKeys(paths: ReadonlySet<string>): Set<string> {
  const ancestors = new Set<string>()
  for (const path of paths) {
    let current = path
    for (;;) {
      const stripped = current.replace(/(\.[^.[\]]+|\[[^\]]*\])$/, '')
      if (stripped === current || stripped.length === 0) break
      ancestors.add(stripped)
      current = stripped
    }
  }
  return ancestors
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

function firstChangedRecordPath(diff: ProjectDiff | null): string | null {
  if (!diff || diff.records.length === 0) return null
  const record = diff.records[0]!
  return record.after?.file_path ?? record.before?.file_path ?? null
}

export function changedTableColumns(projected: ProjectedTable): string[] {
  // 新增和删除记录的全部字段都属于变化；嵌套字段变化投影到顶层列。
  const names = new Set<string>()
  for (const row of projected.data.records) {
    const presentation = projected.presentations.get(row)!
    if (presentation.change === 'modified') {
      for (const name of presentation.changedFields) names.add(name)
    } else {
      for (const field of row.fields) names.add(field.name)
    }
  }
  return projected.data.columns.filter(column => names.has(column.name)).map(column => column.name)
}

// 文件级过滤树——仅保留变化文件及其祖先目录，排序与主树一致；HEAD 独有路径补回同一目录层级。
export function buildDiffTree(diff: ProjectDiff | null, nodes: FileTreeNode[], changed?: ReadonlySet<string>): FileTreeNode[] {
  if (!diff && !changed) return []
  const paths = changed ?? new Set<string>([
    ...((diff?.files ?? []).map(file => file.path)),
    ...((diff?.records ?? []).map(recordPath).filter(path => path.length > 0)),
  ])
  if (paths.size === 0) return []
  // 深拷贝主树一次，后续只在拷贝上补 HEAD 独有路径，避免逐路径全树扫描。
  const clone = (items: FileTreeNode[]): FileTreeNode[] => items.map(node => ({ ...node, children: clone(node.children) }))
  const tree = clone(nodes)
  const indexByPath = new Map<string, FileTreeNode>()
  const collect = (items: FileTreeNode[]): void => {
    for (const node of items) {
      indexByPath.set(node.path, node)
      if (node.children.length > 0) collect(node.children)
    }
  }
  collect(tree)
  // HEAD 独有路径按路径字符串的最长已存在目录前缀挂回同一层级，
  // 兼容目录节点 path 本身含斜杠的情况；无前缀时再逐级补目录。
  const findDir = (dirPath: string): FileTreeNode | undefined => {
    const node = indexByPath.get(dirPath)
    return node && node.is_dir ? node : undefined
  }
  for (const path of paths) {
    if (indexByPath.has(path)) continue
    let parent: FileTreeNode | undefined
    let rest = path
    for (let slash = path.lastIndexOf('/'); slash >= 0; slash = rest.lastIndexOf('/')) {
      const candidate = path.slice(0, slash)
      const dir = findDir(candidate)
      if (dir) {
        parent = dir
        rest = path.slice(slash + 1)
        break
      }
      rest = candidate
    }
    if (parent) {
      const segments = rest.split('/')
      let siblings = parent.children
      let prefix = parent.path
      segments.forEach((name, i) => {
        prefix = `${prefix}/${name}`
        const isDir = i < segments.length - 1
        let node = siblings.find(item => item.path === prefix)
        if (!node) {
          node = { name, path: prefix, is_dir: isDir, in_sources: !isDir, in_schema: !isDir && prefix.endsWith('.cft'), in_data: !isDir && !prefix.endsWith('.cft'), first_source_descendant: isDir ? path : null, children: [] }
          siblings.push(node)
          indexByPath.set(prefix, node)
        }
        siblings = node.children
      })
      continue
    }
    const parts = path.split('/')
    let siblings = tree
    let prefix = ''
    for (let i = 0; i < parts.length; i += 1) {
      prefix = prefix ? `${prefix}/${parts[i]}` : parts[i]!
      const isDir = i < parts.length - 1
      let node = siblings.find(item => item.path === prefix)
      if (!node) {
        node = { name: parts[i]!, path: prefix, is_dir: isDir, in_sources: !isDir, in_schema: !isDir && prefix.endsWith('.cft'), in_data: !isDir && !prefix.endsWith('.cft'), first_source_descendant: isDir ? path : null, children: [] }
        siblings.push(node)
        indexByPath.set(prefix, node)
      }
      siblings = node.children
    }
  }
  const prune = (items: FileTreeNode[]): FileTreeNode[] => items.flatMap(node => {
    const children = prune(node.children)
    if (node.is_dir) return children.length > 0 ? [{ ...node, children }] : []
    return paths.has(node.path) ? [{ ...node, in_sources: true, children: [] }] : []
  }).sort((a, b) => Number(b.is_dir) - Number(a.is_dir) || a.name.localeCompare(b.name))
  return prune(tree)
}

export function recordPath(record: ProjectRecordDiff): string {
  return record.after?.file_path ?? record.before?.file_path ?? ''
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
