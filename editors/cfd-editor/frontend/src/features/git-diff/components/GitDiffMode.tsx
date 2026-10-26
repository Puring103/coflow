import { textSearchMatches } from '../../source-editor/language/textSearch'
import { InspectorPanel } from '../../record-editing/components/InspectorPanel'
import { BuiltinViewPicker } from '../../workspace/components/ViewPicker'
import { ViewToolbar, ViewSearchField } from '../../workspace/components/ViewToolbar'
import { useContentMotion } from '../../../shared/motion/useContentMotion'
import { useSelectionSlide } from '../../../shared/motion/useSelectionSlide'
import { type MotionOrigin } from '../../../shared/motion/motion'
import { valueSelection, recordSelection } from '../../workspace/state/editorSelection'
import { columnContentMetrics } from '../../table/components/TableView.support'
import { useEffect, useMemo, useState, useRef, useCallback } from 'react'
import type { FileRecords } from '../../../bindings/FileRecords'
import type { FileTypeOption } from '../../../bindings/FileTypeOption'
import type { ProjectDiff } from '../../../bindings/ProjectDiff'
import type { ProjectDiffChange } from '../../../bindings/ProjectDiffChange'
import type { ProjectFileDiff } from '../../../bindings/ProjectFileDiff'
import type { ProjectRecordDiff } from '../../../bindings/ProjectRecordDiff'
import type { ProjectRecordSnapshot } from '../../../bindings/ProjectRecordSnapshot'
import type { RecordColumn } from '../../../bindings/RecordColumn'
import type { RecordCoordinate } from '../../../bindings/RecordCoordinate'
import type { RecordRow } from '../../../bindings/RecordRow'
import type { FileTreeNode } from '../../../bindings/FileTreeNode'
import type { DimensionInfo } from '../../../bindings/DimensionInfo'
import { FileTree, buildFileTreeGroups } from '../../workspace/components/FileTree'
import * as api from '../../../api'
import { decodeSemanticTokens } from '../../source-editor/language/lspAdapter'
import { errorMessage } from '../../../wire'
import { summaryOf } from '../../record-editing/value/fieldValue'
import { coordinateId, sameCoordinate, type FieldPathSegment } from '../../../wire'
import type { EditorSelection } from '../../workspace/state/editorSelection'
import { CfdCodeEditor, type CodeLineDecoration, type CodeSemanticToken } from '../../source-editor/components/CfdCodeEditor'
import { Icon } from '../../../shared/ui/Icon'
import { RecordView } from '../../record-editing/components/RecordView'
import { TableView, type TableRowPresentation } from '../../table/components/TableView'
import { changedRecordFields, diffAnnotation, expandedDiffPaths } from '../state/diffPresentation'
import { useDiffRowAlignment } from '../hooks/useDiffRowAlignment'
import './GitDiffMode.css'

export type GitDiffView = 'record' | 'table' | 'source'

export interface GitDiffSelection {
  filePath: string | null
  dimension?: string | null
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
  can_edit_id: false,
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
  const index = useMemo(() => buildDiffIndex(diff), [diff])
  const filtered = useMemo(() => buildDiffTree(diff, nodes, index.changedPaths), [diff, nodes, index])
  // HEAD 独有的覆盖文件也由快照归入所属维度，混合文件可同时出现在基础和维度目录。
  const diffDimensions = useMemo(() => {
    const result = new Map(dimensions.map(d => [d.name, { ...d, files: [...d.files] }]))
    for (const record of diff?.records ?? []) {
      for (const snapshot of [record.before, record.after]) {
        if (!snapshot?.dimension) continue
        let dimension = result.get(snapshot.dimension)
        if (!dimension) {
          dimension = { name: snapshot.dimension, display_name: snapshot.dimension, variants: [], fields: [], dir: null, files: [] }
          result.set(snapshot.dimension, dimension)
        }
        if (!dimension.files.includes(snapshot.file_path)) dimension.files.push(snapshot.file_path)
      }
    }
    return [...result.values()]
  }, [diff, dimensions])
  const groups = useMemo(() => buildFileTreeGroups(filtered, diffDimensions, true)
    .filter(group => group.nodes.length || group.dimensionNodes?.length), [filtered, diffDimensions])
  const types = useMemo(() => Object.fromEntries([...index.changedPaths].map(path => {
    const records = (index.recordsByFile.get(path) ?? []).filter(record => !recordDimension(record))
    const names = [...new Set(records.map(record => record.coordinate.actual_type))]
    return [path, names.map(name => fileTypes[path]?.find(type => type.name === name)
      ?? { name, display_name: name, record_count: records.filter(record => record.coordinate.actual_type === name).length, is_singleton: false, dimension_fields: {} })]
  })), [index, fileTypes])
  const counts = diff ? changeCounts(diff.records) : null
  return <div className="git-changes-sidebar">
    <div className="sidebar-header git-changes-header"><span>Git 变更</span>
      <button className="btn btn-icon" onClick={onRefresh} disabled={loading} title="刷新 Git Diff" aria-label="刷新 Git Diff"><Icon name="refresh" size={14} /></button>
    </div>
    {counts && <div className="git-changes-counts" aria-label="变更统计"><span className="added">+{counts.added}</span><span className="deleted">-{counts.deleted}</span><span className="modified">M{counts.modified}</span></div>}
    {error && <div className="git-diff-message error">{error}</div>}
    {!diff && !error && <div className="git-diff-message">{loading ? '正在比较 HEAD...' : '尚未获得比较结果'}</div>}
    {diff && !hasProjectDiffChanges(diff) && <div className="git-diff-message">工作区与 HEAD 一致</div>}
    <FileTree nodes={filtered} dimensions={diffDimensions} fileTypes={types} groups={groups} readOnly
      selectedFile={selection.dimension ? null : selection.filePath} selectedType={selection.typeName ?? ''}
      selectedDimension={selection.dimension && selection.filePath ? { dimension: selection.dimension, filePath: selection.filePath } : null}
      onSelectFile={(filePath, typeName) => onSelectionChange({ filePath, typeName, coordinate: null })}
      onSelectDimension={target => onSelectionChange({ filePath: target.filePath, dimension: target.dimension, coordinate: null })}
      fileBadge={path => { const change = index.fileByPath.get(path)?.change ?? aggregateChange(index.recordsByFile.get(path) ?? []); return <ChangeMark change={change} /> }} />
  </div>
}

function recordDimension(record: ProjectRecordDiff): string | undefined {
  return record.after?.dimension ?? record.before?.dimension
}

export function GitDiffMode({ diff, loading, error, selection, onSelectionChange, onRefresh, sessionId, fileTypes }: Props & { sessionId: number | null, fileTypes: Record<string, FileTypeOption[] | undefined> }) {
  const index = useMemo(() => buildDiffIndex(diff), [diff])
  const fallbackFile = firstChangedRecordPath(diff) ?? diff?.files[0]?.path ?? null
  const activeFilePath = selection.filePath ?? fallbackFile
  const activeDimension = selection.filePath ? selection.dimension : diff?.records[0] && recordDimension(diff.records[0])
  const typeOptions = (activeFilePath ? fileTypes[activeFilePath] : undefined) ?? []
  const fallbackType = (activeFilePath ? (index.recordsByFile.get(activeFilePath) ?? []).find(record => (recordDimension(record) ?? null) === (activeDimension ?? null))?.coordinate.actual_type : undefined)
    ?? typeOptions[0]?.name
    ?? null
  const changedTypes = [...new Set((activeFilePath ? index.recordsByFile.get(activeFilePath) ?? [] : [])
    .filter(record => (recordDimension(record) ?? null) === (activeDimension ?? null))
    .map(record => record.coordinate.actual_type))]
  const activeTypeName = selection.typeName && changedTypes.includes(selection.typeName) ? selection.typeName : fallbackType
  const isSourceFile = !!activeFilePath && (activeFilePath.endsWith('.cft') || activeFilePath.endsWith('.yaml') || activeFilePath.endsWith('.yml'))
  const recordsForView = useMemo(() => {
    if (!activeFilePath) return []
    const list = (index.recordsByFile.get(activeFilePath) ?? []).filter(record => (recordDimension(record) ?? null) === (activeDimension ?? null))
    if (!activeTypeName) return list
    return list.filter(record => record.coordinate.actual_type === activeTypeName)
  }, [index, activeFilePath, activeTypeName, activeDimension])
  const availableViews: GitDiffView[] = useMemo(() => {
    if (!diff?.semantic_available || !activeFilePath) return ['source']
    if (isSourceFile) return ['source']
    if (recordsForView.length === 0) return ['source']
    return ['table', 'record', 'source']
  }, [diff?.semantic_available, activeFilePath, isSourceFile, recordsForView.length])

  const [queries, setQueries] = useState({ source: '', records: '' })
  const [searchStep, setSearchStep] = useState(0)
  const [view, setView] = useState<GitDiffView>('table')
  // 源码文件的临时显示不覆盖数据视图偏好，切回数据时仍默认表格。
  const clampedView = availableViews.includes(view) ? view : (availableViews[0] ?? 'source')
  const queryKind = clampedView === 'source' ? 'source' : 'records'
  const query = queries[queryKind]
  const setQuery = (value: string) => setQueries(current => ({ ...current, [queryKind]: value }))
  // 记录/表格视图共用的字段过滤开关，默认仅显示修改字段。
  const [changedOnly, setChangedOnly] = useState(true)
  const contentRef = useRef<HTMLDivElement>(null)
  useContentMotion(contentRef, JSON.stringify([activeFilePath, clampedView, activeDimension, activeTypeName]))

  // 项目加载失败时仍允许切换标签，但不能读取会话或显示旧项目的差异。
  if (sessionId === null) return <ModeMessage text="请先成功打开项目后查看 Git 变更" onRefresh={onRefresh} />
  if (error) return <ModeMessage tone="error" text={error} onRefresh={onRefresh} />
  if (!diff) return <ModeMessage text={loading ? '正在比较 HEAD...' : '尚未获得比较结果'} onRefresh={onRefresh} />
  if (!hasProjectDiffChanges(diff)) return <ModeMessage text="工作区与 HEAD 一致" onRefresh={onRefresh} />
  if (!activeFilePath) return <ModeMessage text="没有可显示的变化文件" onRefresh={onRefresh} />

  const activeFile = index.fileByPath.get(activeFilePath) ?? diff.files.find(file => file.path === activeFilePath) ?? null
  const searchTotal = textSearchMatches(activeFile?.before ?? '', query).length + textSearchMatches(activeFile?.after ?? '', query).length
  const searchCurrent = searchTotal ? ((searchStep % searchTotal) + searchTotal) % searchTotal + 1 : 0
  return (
    <section className="git-diff-mode git-diff-selectable">
      <ViewToolbar className="git-diff-mode-toolbar">
        <span className="git-diff-head">HEAD {diff.head_oid.slice(0, 12)}</span>
        <BuiltinViewPicker views={availableViews} active={clampedView} onSelect={setView} />
        <ViewSearchField value={query} onChange={value => { setQuery(value); setSearchStep(0) }}
          label={clampedView === 'source' ? '搜索对比文本' : '筛选变化记录'} placeholder={clampedView === 'source' ? '搜索本文件文本…' : '筛选记录…'}
          onKeyDown={event => {
            if (event.key === 'Escape') setQuery('')
            if (event.key === 'Enter' && clampedView === 'source') { event.preventDefault(); setSearchStep(step => step + (event.shiftKey ? -1 : 1)) }
          }} />
        {clampedView === 'source' && <>
          <span className="global-search-count" aria-live="polite">{searchCurrent} / {searchTotal}</span>
          <div className="source-search-actions">
            <button className="btn btn-icon" aria-label="上一个匹配" disabled={!searchTotal} onClick={() => setSearchStep(step => step - 1)}><Icon name="arrow-left" size={14} /></button>
            <button className="btn btn-icon" aria-label="下一个匹配" disabled={!searchTotal} onClick={() => setSearchStep(step => step + 1)}><Icon name="chevron-right" size={14} /></button>
          </div>
        </>}
        {activeDimension && changedTypes.length > 1 && <select aria-label="对比类型" value={activeTypeName ?? ''}
          onChange={event => onSelectionChange({ filePath: activeFilePath, dimension: activeDimension, typeName: event.target.value, coordinate: null })}>
          {changedTypes.map(name => <option key={name} value={name}>{typeOptions.find(type => type.name === name)?.display_name ?? name}</option>)}
        </select>}
        {(clampedView === 'record' || clampedView === 'table') && (
          <label className="git-diff-changed-only"><input type="checkbox" checked={changedOnly} onChange={event => setChangedOnly(event.target.checked)} />仅修改字段</label>
        )}
        {loading && <span className="git-diff-updating">正在更新...</span>}
        <button className="btn btn-icon" onClick={onRefresh} disabled={loading} title="刷新 Git Diff" aria-label="刷新 Git Diff">
          <Icon name="refresh" size={14} aria-hidden />
        </button>
      </ViewToolbar>
      <div ref={contentRef} className="view-container">
      {clampedView === 'record' ? (
        <RecordDiffView search={query} records={recordsForView} selection={selection} onSelectionChange={onSelectionChange} changedOnly={changedOnly} />
      ) : clampedView === 'table' ? (
        <TableComparison search={query} diff={diff} dimension={activeDimension} filePath={activeFilePath} actualType={activeTypeName ?? ''} changedOnly={changedOnly} />
      ) : (
        <SourceComparison file={activeFile} sessionId={sessionId} searchQuery={query} searchStep={searchStep} />
      )}
      </div>
    </section>
  )
}

// 记录视图左侧只列变化记录（支持搜索定位），右侧并排显示选中记录的 HEAD/当前对比。
function RecordDiffView({ search, records, selection, onSelectionChange, changedOnly }: {
  search: string
  records: ProjectRecordDiff[]
  selection: GitDiffSelection
  onSelectionChange(selection: GitDiffSelection): void
  changedOnly: boolean
}) {
  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase()
    if (!query) return records
    return records.filter(record => `${record.coordinate.actual_type}.${record.coordinate.id}`.toLowerCase().includes(query))
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
  const changedPaths = useMemo(() => new Set(activeRecord?.fields.map(field => field.path) ?? []), [activeRecord])
  // 仅修改字段模式下，记录窗格只保留变化顶层字段，并自动展开其祖先路径。
  const visibleTopFields = useMemo(() => {
    if (!changedOnly || !activeRecord || activeRecord.change !== 'modified') return undefined
    const fields = changedRecordFields(activeRecord)
    return fields.size > 0 ? fields : undefined
  }, [changedOnly, activeRecord])
  const autoExpandPaths = useMemo(() => activeRecord ? expandedDiffPaths(activeRecord) : new Set<string>(), [activeRecord])
  const [expansion, setExpansion] = useState<{ record: ProjectRecordDiff | null, paths: ReadonlySet<string> } | null>(null)
  const expandedPaths = expansion?.record === activeRecord ? expansion.paths : autoExpandPaths
  const onToggleExpandedPath = (path: string, expanded: boolean) => {
    const paths = new Set(expandedPaths)
    if (expanded) paths.add(path)
    else paths.delete(path)
    setExpansion({ record: activeRecord, paths })
  }
  const listRef = useRef<HTMLDivElement>(null)
  useSelectionSlide(listRef, '[aria-selected="true"]')
  const comparisonRef = useRef<HTMLDivElement>(null)
  useDiffRowAlignment(comparisonRef, activeRecord)
  if (records.length === 0) return <div className="git-diff-message centered">没有可显示的记录变化</div>
  return (
    <div className="git-record-diff">
      <aside className="git-record-list" aria-label="变化记录">
        <div ref={listRef} className="git-record-list-items rv-sidebar" role="listbox" aria-label="变化记录列表">
          {filtered.map(record => {
            const selected = !!activeRecord && sameCoordinate(activeRecord.coordinate, record.coordinate)
            return (
              <button key={coordinateId(record.coordinate)} role="option" aria-selected={selected}
                className={`rv-sidebar-item git-record-list-item${selected ? ' selected' : ''}`}
                onClick={() => onSelectionChange({ ...selection, coordinate: record.coordinate })}>
                <span className="tree-item-label" title={`${record.coordinate.actual_type}.${record.coordinate.id}`}>{record.coordinate.actual_type}.{record.coordinate.id}</span>
                <ChangeMark change={record.change} />
              </button>
            )
          })}
          {filtered.length === 0 && <div className="git-diff-message">无匹配记录</div>}
        </div>
      </aside>
      <div ref={comparisonRef} className={`git-record-comparison ${activeRecord?.change ?? ''}`}>
        <RecordPane label="HEAD" snapshot={activeRecord?.before} record={activeRecord} changedPaths={changedPaths} visibleTopFields={visibleTopFields} expandedPaths={expandedPaths} onToggleExpandedPath={onToggleExpandedPath} />
        <RecordPane label="当前工作区" snapshot={activeRecord?.after} record={activeRecord} changedPaths={changedPaths} visibleTopFields={visibleTopFields} expandedPaths={expandedPaths} onToggleExpandedPath={onToggleExpandedPath} />
      </div>
    </div>
  )
}

function RecordPane({ label, snapshot, record, changedPaths, visibleTopFields, expandedPaths, onToggleExpandedPath }: {
  label: string
  snapshot?: ProjectRecordSnapshot
  record: ProjectRecordDiff | null
  changedPaths: ReadonlySet<string>
  visibleTopFields?: ReadonlySet<string>
  expandedPaths: ReadonlySet<string>
  onToggleExpandedPath(path: string, expanded: boolean): void
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
        expandedPaths={expandedPaths}
        onToggleExpandedPath={onToggleExpandedPath}
      />
    </section>
  )
}

// 表格 diff——新增/删除单行（整行绿/红），修改显示两行（先原本 HEAD，后修改后当前），变化单元格深黄高亮。
function TableComparison({ search, diff, dimension, filePath, actualType, changedOnly }: {
  search: string
  diff: ProjectDiff
  dimension?: string | null
  filePath: string | null
  actualType: string
  changedOnly: boolean
}) {
  const [selection, setSelection] = useState<EditorSelection | null>(null)
  const [expandedCell, setExpandedCell] = useState<{ data: FileRecords; selection: EditorSelection; origin: MotionOrigin } | null>(null)
  useEffect(() => setExpandedCell(null), [filePath, actualType, dimension])
  const projected = useMemo(
    () => projectTable(diff, filePath, actualType, dimension),
    [diff, filePath, actualType, dimension],
  )
  const [expansion, setExpansion] = useState<{ projected: ProjectedTable, paths: ReadonlyMap<string, ReadonlySet<string>> } | null>(null)
  const rowPresentation = useCallback((row: RecordRow): TableRowPresentation | undefined => {
    const presentation = projected.presentations.get(row)
    if (!presentation) return undefined
    const id = coordinateId(row.coordinate)
    return { ...presentation,
      expandedPaths: (expansion?.projected === projected ? expansion.paths.get(id) : undefined) ?? presentation.expandedPaths,
      onToggleExpandedPath: (path, expanded) => setExpansion(current => {
        const paths = new Map(current?.projected === projected ? current.paths : [])
        const recordPaths = new Set(paths.get(id) ?? presentation.expandedPaths)
        if (expanded) recordPaths.add(path)
        else recordPaths.delete(path)
        paths.set(id, recordPaths)
        return { projected, paths }
      }),
    }
  }, [projected, expansion])
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
      searchQuery={search}
      activeType={actualType}
      readOnly
      selection={selection}
      onSelectValue={(coordinate, fieldPath) => setSelection({ kind: 'value', filePath: projected.data.file_path, coordinate, fieldPath, rangeAnchor: { coordinate, fieldPath } })}
      onSelectRecord={(coordinate) => setSelection({ kind: 'record', filePath: projected.data.file_path, coordinate, coordinates: [coordinate], anchor: coordinate })}
      onClearSelection={() => setSelection(null)}
      visibleColumns={visible}
      onOpenRecord={() => {}}
      rowPresentation={rowPresentation}
      onExpandCell={(coordinate, path, origin) => {
        const row = projected.data.records.find(item => sameCoordinate(item.coordinate, coordinate))
        if (!row) return
        const original = projected.presentations.get(row)!.coordinate!
        setSelection(path ? valueSelection(projected.data.file_path, coordinate, path) : recordSelection(projected.data.file_path, coordinate))
        setExpandedCell({ data: { ...projected.data, records: [{ ...row, coordinate: original }] },
          selection: path ? valueSelection(projected.data.file_path, original, path) : recordSelection(projected.data.file_path, original), origin })
      }}
    />
    <InspectorPanel modal open={!!expandedCell} data={expandedCell?.data ?? null} selection={expandedCell?.selection ?? null}
      origin={expandedCell?.origin} readOnly width={420} onWidthChange={() => {}} onClose={() => setExpandedCell(null)} />
    </div>
  )
}

function SourceComparison({ file, sessionId, searchQuery, searchStep }: { file: ProjectFileDiff | null, sessionId: number, searchQuery: string, searchStep: number }) {
  const beforeCount = textSearchMatches(file?.before ?? '', searchQuery).length
  const total = beforeCount + textSearchMatches(file?.after ?? '', searchQuery).length
  const current = total ? ((searchStep % total) + total) % total : -1
  const decorations = useMemo(() => file ? sourceLineDecorations(file.patch) : { before: [], after: [] }, [file])
  if (!file) return <div className="git-diff-message centered">没有可显示的源码变化</div>
  return (
    <div className={`git-source-comparison ${file.change}`}>
      <SourcePane searchQuery={searchQuery} searchStep={current} searchActive={current >= 0 && current < beforeCount} sessionId={sessionId} filePath={file.path} label="HEAD" source={file.before} missing="HEAD 中不存在" decorations={decorations.before} />
      <SourcePane searchQuery={searchQuery} searchStep={current - beforeCount} searchActive={current >= beforeCount} sessionId={sessionId} filePath={file.path} label="当前工作区" source={file.after} missing="当前工作区中不存在" decorations={decorations.after} />
    </div>
  )
}

function SourcePane({ label, source, missing, decorations, sessionId, filePath, searchQuery, searchStep, searchActive }: {
  searchActive: boolean
  searchQuery: string
  searchStep: number
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
        : <CfdCodeEditor searchActive={searchActive} searchQuery={searchQuery} searchStep={searchStep} value={source} onChange={() => {}} readOnly semanticTokens={highlight?.source === source && highlight.filePath === filePath ? highlight.tokens : []} lineDecorations={decorations} />}
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
export function projectTable(diff: ProjectDiff, filePath: string | null, actualType: string, dimension?: string | null): ProjectedTable {
  const rows: RecordRow[] = []
  const presentations = new Map<RecordRow, TableRowPresentation>()
  if (!actualType) {
    return {
      data: { revision: diff.target_revision, file_path: filePath ?? '', type_names: [], columns: [], records: [], capabilities: READ_ONLY_CAPABILITIES },
      presentations,
    }
  }
  for (const record of diff.records) {
    if ((recordDimension(record) ?? null) !== (dimension ?? null)) continue
    if (record.coordinate.actual_type !== actualType) continue
    if (filePath && recordPath(record) !== filePath) continue
    if (record.change === 'modified' && record.before && record.after) {
      const changedFields = changedRecordFields(record)
      const expandedPaths = expandedDiffPaths(record)
      const changedPaths = new Set(record.fields.map(field => field.path))
      const beforeRow = snapshotRow(record, record.before, rows.length, 0)
      rows.push(beforeRow)
      presentations.set(beforeRow, {
        id: `${coordinateId(record.coordinate)}:before`,
        version: 'HEAD',
        change: record.change,
        changedFields,
        expandedPaths,
        changedPaths,
        groupStart: true,
      })
      const afterRow = snapshotRow(record, record.after, rows.length, 0)
      rows.push(afterRow)
      presentations.set(afterRow, {
        id: `${coordinateId(record.coordinate)}:after`,
        version: '当前',
        change: record.change,
        changedFields,
        expandedPaths,
        changedPaths,
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
      changedFields: changedRecordFields(record),
      expandedPaths: expandedDiffPaths(record),
      changedPaths: new Set(record.fields.map(field => field.path)),
      groupStart: true,
    })
  }
  const file = filePath ?? rows[0]?.display_path ?? ''
  const columns = columnsFor(rows, actualType)
  const sizedRows = rows.map((row, index) => ({ ...row,
    coordinate: { ...row.coordinate, id: presentations.get(row)!.id },
    container_index: index, container_size: rows.length,
  }))
  // 行元数据使用对象身份关联，替换容器位置时同步重建索引。
  const sizedPresentations = new Map<RecordRow, TableRowPresentation>()
  rows.forEach((row, index) => sizedPresentations.set(sizedRows[index]!, { ...presentations.get(row)!, coordinate: row.coordinate }))
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
    ? snapshot.values.filter(item => visibleTopFields.has(item.path))
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

function snapshotRow(record: ProjectRecordDiff, snapshot: ProjectRecordSnapshot, index: number, size: number): RecordRow {
  const fields = snapshot.values.map(item => ({ name: item.path, value: item.value, missing: false, annotation: diffAnnotation(item.display) }))
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
    content_metrics: columnContentMetrics(rows, name),
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
  const currentPaths = new Set(indexByPath.keys())
  // HEAD 独有路径按路径字符串的最长已存在目录前缀挂回同一层级，
  // 兼容目录节点 path 本身含斜杠（如维度 out_dir）的情况；无前缀时再逐级补目录。
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
          node = { name, path: prefix, is_dir: isDir, in_sources: !isDir, in_schema: !isDir && prefix.endsWith('.cft'), in_data: !isDir && !prefix.endsWith('.cft'), dimension_only: false, first_source_descendant: isDir ? path : null, children: [] }
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
        node = { name: parts[i]!, path: prefix, is_dir: isDir, in_sources: !isDir, in_schema: !isDir && prefix.endsWith('.cft'), in_data: !isDir && !prefix.endsWith('.cft'), dimension_only: false, first_source_descendant: isDir ? path : null, children: [] }
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
  const dimensionPaths = new Set<string>()
  const basePaths = new Set<string>()
  for (const record of diff?.records ?? []) {
    for (const snapshot of [record.before, record.after]) {
      if (snapshot) (snapshot.dimension ? dimensionPaths : basePaths).add(snapshot.file_path)
    }
  }
  for (const path of dimensionPaths) {
    const node = indexByPath.get(path)
    if (node && !basePaths.has(path) && !currentPaths.has(path)) node.dimension_only = true
  }
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
