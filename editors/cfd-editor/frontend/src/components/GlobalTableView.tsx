import React, { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { RecordRow, FieldValue, FieldPathSegment, DiagnosticItem, FieldSchema } from "../bindings";
import type { Route } from "../router";
import { DataCard } from "./DataCard";
import { ContextMenu, type ContextMenuState } from "./ContextMenu";
import { api } from "../api";

interface CellEditorProps {
  value: FieldValue;
  sessionId: number;
  onCommit: (raw: string) => void;
  onCancel: () => void;
  onTabCommit?: (raw: string) => void;
  onShiftTabCommit?: (raw: string) => void;
}

const CELL_EDITOR_STYLE: React.CSSProperties = {
  width: "100%",
  height: "100%",
  padding: "4px 8px",
  background: "var(--bg3)",
  border: "1px solid var(--accent)",
  borderRadius: 0,
  color: "var(--text)",
  fontSize: 12,
  fontFamily: "monospace",
  outline: "none",
  boxSizing: "border-box",
};

function fieldValueToStringEditor(v: FieldValue): string {
  switch (v.kind) {
    case "Null": return "";
    case "Bool": return String(v.v);
    case "Int": case "Float": return String(v.v);
    case "Str": return v.v;
    case "Enum": return v.variant;
    case "Ref": return v.target_key;
    default: return "";
  }
}

function CellEditor({ value, sessionId, onCommit, onCancel, onTabCommit, onShiftTabCommit }: CellEditorProps) {
  const [text, setText] = useState(() => fieldValueToStringEditor(value));
  const inputRef = useRef<HTMLInputElement>(null);
  const [enumVariants, setEnumVariants] = useState<string[] | null>(null);
  const [refTargets, setRefTargets] = useState<string[]>([]);
  const listId = useRef(`gcl-${Math.random().toString(36).slice(2)}`).current;

  useEffect(() => {
    if (value.kind === "Enum") {
      api.getEnumVariants(sessionId, value.enum_name).then(vs => {
        setEnumVariants(vs.length > 0 ? vs : []);
      }).catch(() => setEnumVariants([]));
    } else if (value.kind === "Ref" && value.target_type) {
      api.getRefTargets(sessionId, value.target_type).then(keys => {
        if (keys.length > 0) setRefTargets(keys);
      }).catch(() => {});
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, value.kind === "Enum" ? (value as { enum_name: string }).enum_name : "",
      value.kind === "Ref" ? (value as { target_type: string }).target_type : ""]);

  useEffect(() => {
    if (value.kind !== "Enum") {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [value.kind]);

  if (value.kind === "Enum") {
    if (enumVariants === null) {
      return <div style={{ ...CELL_EDITOR_STYLE, color: "var(--text-muted)", fontStyle: "italic" }}>Loading…</div>;
    }
    if (enumVariants.length > 0) {
      return (
        <select
          value={text}
          onChange={e => setText(e.target.value)}
          onBlur={e => onCommit(e.currentTarget.value)}
          onKeyDown={e => {
            if (e.key === "Enter") { e.preventDefault(); onCommit(e.currentTarget.value); }
            if (e.key === "Escape") { e.preventDefault(); onCancel(); }
            if (e.key === "Tab") { e.preventDefault(); if (e.shiftKey) (onShiftTabCommit ?? onCommit)(e.currentTarget.value); else (onTabCommit ?? onCommit)(e.currentTarget.value); }
            e.stopPropagation();
          }}
          autoFocus
          style={CELL_EDITOR_STYLE}
        >
          {enumVariants.map(v => <option key={v} value={v}>{v}</option>)}
        </select>
      );
    }
  }

  if (value.kind === "Ref") {
    return (
      <>
        {refTargets.length > 0 && (
          <datalist id={listId}>
            {refTargets.map(k => <option key={k} value={k} />)}
          </datalist>
        )}
        <input
          ref={inputRef}
          value={text}
          list={refTargets.length > 0 ? listId : undefined}
          onChange={e => setText(e.target.value)}
          onBlur={() => { const t = text.trim(); if (t && t !== value.target_key) onCommit(text); else onCancel(); }}
          onKeyDown={e => {
            if (e.key === "Enter") { e.preventDefault(); if (text.trim()) onCommit(text); else onCancel(); }
            if (e.key === "Escape") { e.preventDefault(); onCancel(); }
            if (e.key === "Tab") { e.preventDefault(); if (e.shiftKey) { if (text.trim()) (onShiftTabCommit ?? onCommit)(text); else onCancel(); } else { if (text.trim()) (onTabCommit ?? onCommit)(text); else onCancel(); } }
            e.stopPropagation();
          }}
          style={CELL_EDITOR_STYLE}
          placeholder="record_key"
        />
      </>
    );
  }

  return (
    <input
      ref={inputRef}
      value={text}
      onChange={e => setText(e.target.value)}
      onBlur={() => onCommit(text)}
      onKeyDown={e => {
        if (e.key === "Enter") { e.preventDefault(); onCommit(text); }
        if (e.key === "Escape") { e.preventDefault(); onCancel(); }
        if (e.key === "Tab") { e.preventDefault(); if (e.shiftKey) (onShiftTabCommit ?? onCommit)(text); else (onTabCommit ?? onCommit)(text); }
        e.stopPropagation();
      }}
      style={CELL_EDITOR_STYLE}
    />
  );
}

interface GlobalTableViewProps {
  sessionId: number;
  typeName: string;
  refreshKey?: number;
  onTypeChange: (typeName: string) => void;
  onNavigate: (route: Route) => void;
  onWriteField?: (sessionId: number, filePath: string, recordKey: string, fieldPath: FieldPathSegment[], newValue: FieldValue, oldValue?: FieldValue) => Promise<void>;
  onDeleteRecord?: (sessionId: number, filePath: string, recordKey: string) => Promise<void>;
  onDuplicateRecord?: (sessionId: number, filePath: string, srcKey: string, newKey: string) => Promise<void>;
  onMoveRecord?: (srcFile: string, recordKey: string) => void;
  onCopyRecord?: (srcFile: string, recordKey: string) => void;
  onCreateRecord?: (typeName: string, filePath: string, key: string) => Promise<void>;
  onImportRecord?: (filePath: string, source: string) => Promise<string[]>;
  availableFiles?: string[];
  diagnostics?: DiagnosticItem[];
  onError?: (msg: string) => void;
  onSuccess?: (msg: string) => void;
}

function fieldValueToJson(v: FieldValue): unknown {
  switch (v.kind) {
    case "Null": return null;
    case "Bool": return v.v;
    case "Int": case "Float": return v.v;
    case "Str": return v.v;
    case "Enum": return v.variant;
    case "Ref": return `&${v.target_key}`;
    case "Object": { const o: Record<string, unknown> = { _type: v.actual_type }; for (const f of v.fields) o[f.name] = fieldValueToJson(f.value); return o; }
    case "Array": return v.items.map(fieldValueToJson);
    case "Dict": { const o: Record<string, unknown> = {}; for (const e of v.entries) { const k = e.key.kind === "Str" ? e.key.v : e.key.kind === "Int" ? String(e.key.v) : e.key.variant; o[k] = fieldValueToJson(e.value); } return o; }
    default: return null;
  }
}

function parseFieldValue(raw: string, original: FieldValue): FieldValue {
  const t = raw.trim();
  if (original.kind === "Bool") return { kind: "Bool", v: t === "true" };
  if (original.kind === "Int") { const n = Number(t); return isNaN(n) ? original : { kind: "Int", v: n }; }
  if (original.kind === "Float") { const n = parseFloat(t); return isNaN(n) ? original : { kind: "Float", v: n }; }
  if (original.kind === "Enum") return { kind: "Enum", enum_name: original.enum_name, variant: t, int_value: original.int_value };
  if (original.kind === "Ref") return t ? { kind: "Ref", target_type: original.target_type, target_key: t, target_file: null } : original;
  if (original.kind === "Null" && (t === "true" || t === "false")) return { kind: "Bool", v: t === "true" };
  if (original.kind === "Null") { const n = Number(t); if (!isNaN(n) && t !== "") return { kind: "Int", v: n }; }
  return { kind: "Str", v: raw };
}

function fieldValueToString(v: FieldValue): string {
  switch (v.kind) {
    case "Null": return "";
    case "Bool": return String(v.v);
    case "Int": case "Float": return String(v.v);
    case "Str": return v.v;
    case "Enum": return v.variant;
    case "Ref": return v.target_key;
    default: return "";
  }
}

type SortCol = { col: "key" | "file" | string; dir: "asc" | "desc" };

export function GlobalTableView({ sessionId, typeName, refreshKey, onTypeChange, onNavigate, onWriteField, onDeleteRecord, onDuplicateRecord, onMoveRecord, onCopyRecord, onCreateRecord, onImportRecord, availableFiles, diagnostics, onError, onSuccess }: GlobalTableViewProps) {
  const [rows, setRows] = useState<RecordRow[]>([]);
  const [allTypeNames, setAllTypeNames] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortCol | null>(() => {
    try {
      const stored = localStorage.getItem(`cfd-global-sort:${typeName}`);
      return stored ? JSON.parse(stored) : null;
    } catch { return null; }
  });
  const [focusedIdx, setFocusedIdx] = useState<number | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [batchField, setBatchField] = useState("");
  const [batchValue, setBatchValue] = useState("");
  const [batchApplying, setBatchApplying] = useState(false);
  const [batchError, setBatchError] = useState<string | null>(null);
  const [batchSuccess, setBatchSuccess] = useState<string | null>(null);
  const batchSuccessTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [batchDeletePending, setBatchDeletePending] = useState(false);
  const [singleDeleteModal, setSingleDeleteModal] = useState<{ key: string; filePath: string } | null>(null);
  const [duplicateModal, setDuplicateModal] = useState<{ srcKey: string; filePath: string; draft: string; error: string | null } | null>(null);
  const [typeCounts, setTypeCounts] = useState<Map<string, number>>(new Map());
  const [createModal, setCreateModal] = useState<{ key: string; filePath: string; creating: boolean; error: string | null } | null>(null);
  const [pasteModal, setPasteModal] = useState<{ source: string; filePath: string; importing: boolean; error: string | null; importedKeys?: string[] } | null>(null);
  const [editingCell, setEditingCell] = useState<{ rowKey: string; filePath: string; fieldName: string; value: FieldValue } | null>(null);
  const [hiddenCols, setHiddenCols] = useState<Set<string>>(() => {
    try {
      const stored = localStorage.getItem(`cfd-global-col-vis:${typeName}`);
      return stored ? new Set<string>(JSON.parse(stored)) : new Set<string>();
    } catch { return new Set<string>(); }
  });
  const [showColPicker, setShowColPicker] = useState(false);
  const colPickerRef = useRef<HTMLDivElement>(null);
  const [fieldSchemas, setFieldSchemas] = useState<FieldSchema[]>([]);
  const [showRequiredOnly, setShowRequiredOnly] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [colWidths, setColWidths] = useState<Record<string, number>>(() => {
    try {
      const stored = localStorage.getItem(`cfd-global-col-width:${typeName}`);
      return stored ? JSON.parse(stored) : {};
    } catch { return {}; }
  });

  useEffect(() => {
    api.getAllTypeNames(sessionId).then(names => setAllTypeNames(names)).catch(e => onError?.(`加载类型列表失败: ${e}`));
  }, [sessionId, onError]);

  // Load record counts per type for tab badges
  useEffect(() => {
    api.getAllRecordsBrief(sessionId).then(briefs => {
      const counts = new Map<string, number>();
      for (const b of briefs) counts.set(b.actual_type, (counts.get(b.actual_type) ?? 0) + 1);
      setTypeCounts(counts);
    }).catch(() => {});
  // refreshKey triggers re-count when records change
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, refreshKey]);

  useEffect(() => {
    if (!typeName) { setFieldSchemas([]); return; }
    api.getFieldSchemas(sessionId, typeName)
      .then(s => setFieldSchemas(s))
      .catch(() => setFieldSchemas([]));
  }, [sessionId, typeName]);

  useEffect(() => {
    if (!typeName) return;
    setLoading(true);
    setError(null);
    // Restore persisted sort for the newly selected type
    try {
      const stored = localStorage.getItem(`cfd-global-sort:${typeName}`);
      setSort(stored ? JSON.parse(stored) : null);
    } catch { setSort(null); }
    api.getAllRecordsOfType(sessionId, typeName)
      .then(r => { setRows(r); setLoading(false); })
      .catch(e => { setError(String(e)); setLoading(false); });
  // refreshKey is intentionally included to allow callers to force a re-fetch
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, typeName, refreshKey]);

  const requiredFieldNames = useMemo(
    () => new Set(fieldSchemas.filter(s => !s.has_default).map(s => s.name)),
    [fieldSchemas],
  );

  const filteredRows = useMemo(() => {
    const base = rows.filter(r => {
      if (showRequiredOnly) {
        const hasRequiredNull = r.fields.some(f => requiredFieldNames.has(f.name) && f.value.kind === "Null");
        if (!hasRequiredNull) return false;
      }
      if (!search) return true;
      const q = search.toLowerCase();
      // Support field:value syntax (and file:name, key:name)
      const colonIdx = search.indexOf(":");
      if (colonIdx > 0) {
        const fname = search.slice(0, colonIdx).trim().toLowerCase();
        const fval = search.slice(colonIdx + 1).trim().toLowerCase();
        if (fname === "key") return r.key.toLowerCase().includes(fval);
        if (fname === "file") return r.file_path.toLowerCase().includes(fval);
        const cell = r.fields.find(f => f.name.toLowerCase() === fname);
        if (cell) return fieldValueToString(cell.value).toLowerCase().includes(fval);
      }
      if (r.key.toLowerCase().includes(q)) return true;
      if (r.file_path.toLowerCase().includes(q)) return true;
      return r.fields.some(f => fieldValueToString(f.value).toLowerCase().includes(q));
    });
    if (!sort) return base;
    return [...base].sort((a, b) => {
      let av: string, bv: string;
      if (sort.col === "key") { av = a.key; bv = b.key; }
      else if (sort.col === "file") {
        av = a.file_path.split(/[\\/]/).pop() ?? a.file_path;
        bv = b.file_path.split(/[\\/]/).pop() ?? b.file_path;
      } else {
        av = fieldValueToString(a.fields.find(f => f.name === sort.col)?.value ?? { kind: "Null" });
        bv = fieldValueToString(b.fields.find(f => f.name === sort.col)?.value ?? { kind: "Null" });
      }
      const cmp = av.localeCompare(bv, undefined, { numeric: true, sensitivity: "base" });
      return sort.dir === "asc" ? cmp : -cmp;
    });
  }, [rows, search, sort, showRequiredOnly, requiredFieldNames]);

  // Per-record diagnostic counts for row badges
  const rowDiagCounts = useMemo(() => {
    if (!diagnostics) return new Map<string, { errors: number; warnings: number }>();
    const map = new Map<string, { errors: number; warnings: number }>();
    for (const d of diagnostics) {
      if (!d.record_key) continue;
      const entry = map.get(d.record_key) ?? { errors: 0, warnings: 0 };
      if (d.severity === "error") entry.errors++;
      else if (d.severity === "warning") entry.warnings++;
      map.set(d.record_key, entry);
    }
    return map;
  }, [diagnostics]);

  const handleSortClick = (col: string) => {
    setSort(s => {
      const next = s?.col === col ? (s.dir === "asc" ? { col, dir: "desc" as const } : null) : { col, dir: "asc" as const };
      try {
        if (next) localStorage.setItem(`cfd-global-sort:${typeName}`, JSON.stringify(next));
        else localStorage.removeItem(`cfd-global-sort:${typeName}`);
      } catch { /* ignore */ }
      return next;
    });
  };

  // Determine field names from union of all records
  const fieldNames = useMemo(() => {
    const seen = new Set<string>();
    const names: string[] = [];
    for (const r of rows) {
      for (const f of r.fields) {
        if (!seen.has(f.name)) { seen.add(f.name); names.push(f.name); }
      }
    }
    return names;
  }, [rows]);

  const visibleFieldNames = useMemo(
    () => fieldNames.filter(f => !hiddenCols.has(f)),
    [fieldNames, hiddenCols],
  );

  const COL_KEY = colWidths["__key__"] ?? 120;
  const COL_FILE = colWidths["__file__"] ?? 140;
  const COL_FIELD_DEFAULT = 120;
  const getColWidth = (name: string) => colWidths[name] ?? COL_FIELD_DEFAULT;
  const ROW_H = 36;

  const handleColResizeMouseDown = useCallback((colId: string, e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = colId === "__key__" ? COL_KEY : colId === "__file__" ? COL_FILE : getColWidth(colId);
    const onMouseMove = (ev: MouseEvent) => {
      const newW = Math.max(48, startWidth + ev.clientX - startX);
      setColWidths(prev => {
        const next = { ...prev, [colId]: newW };
        try { localStorage.setItem(`cfd-global-col-width:${typeName}`, JSON.stringify(next)); } catch { /* ignore */ }
        return next;
      });
    };
    const onMouseUp = () => {
      document.removeEventListener("mousemove", onMouseMove);
      document.removeEventListener("mouseup", onMouseUp);
    };
    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
  }, [COL_KEY, COL_FILE, colWidths, typeName, getColWidth]);

  const virtualizer = useVirtualizer({
    count: filteredRows.length,
    getScrollElement: () => containerRef.current,
    estimateSize: () => ROW_H,
    overscan: 10,
  });

  const handleRowClick = useCallback((row: RecordRow) => {
    onNavigate({ view: "record", file: row.file_path, recordKey: row.key });
  }, [onNavigate]);

  const handleRowContextMenu = useCallback((e: React.MouseEvent, row: RecordRow) => {
    e.preventDefault();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "跳转到记录视图", onClick: () => onNavigate({ view: "record", file: row.file_path, recordKey: row.key }) },
        { label: "在文件表视图中打开", onClick: () => onNavigate({ view: "table", file: row.file_path }) },
        { label: "在资源管理器中显示", onClick: () => api.revealInExplorer(sessionId, row.file_path).catch(e => onError?.(`无法打开资源管理器: ${e}`)) },
        { label: "复制 Key", onClick: () => navigator.clipboard.writeText(row.key).then(() => onSuccess?.(`已复制 Key: ${row.key}`)).catch(e => onError?.(`复制失败: ${e}`)) },
        { label: "复制为 CFD 源码", onClick: () => api.getRecordSource(sessionId, row.file_path, row.key).then(src => navigator.clipboard.writeText(src)).then(() => onSuccess?.("已复制为 CFD 源码")).catch(e => onError?.(`复制失败: ${e}`)) },
        { label: "复制为 JSON", onClick: () => { const obj: Record<string, unknown> = { _key: row.key, _type: row.actual_type }; for (const f of row.fields) obj[f.name] = fieldValueToJson(f.value); navigator.clipboard.writeText(JSON.stringify(obj, null, 2)).then(() => onSuccess?.("已复制为 JSON")).catch(e => onError?.(`复制失败: ${e}`)); } },
        ...(onDuplicateRecord ? [{ label: "复制记录 (Ctrl+D)", onClick: () => setDuplicateModal({ srcKey: row.key, filePath: row.file_path, draft: `${row.key}_copy`, error: null }) }] : []),
        ...(onMoveRecord ? [{ label: "移动到文件…", onClick: () => onMoveRecord(row.file_path, row.key) }] : []),
        ...(onCopyRecord ? [{ label: "复制到文件…", onClick: () => onCopyRecord(row.file_path, row.key) }] : []),
        ...(onDeleteRecord ? [{ label: "删除记录", danger: true as const, onClick: () => onDeleteRecord(sessionId, row.file_path, row.key).catch(e => onError?.(`删除失败: ${e}`)) }] : []),
      ],
    });
  }, [sessionId, onNavigate, onDeleteRecord, onMoveRecord, onCopyRecord, onError]);

  const showBatchSuccess = useCallback((msg: string) => {
    setBatchSuccess(msg);
    setBatchError(null);
    if (batchSuccessTimerRef.current) clearTimeout(batchSuccessTimerRef.current);
    batchSuccessTimerRef.current = setTimeout(() => setBatchSuccess(null), 3000);
  }, []);

  const handleBatchApply = useCallback(async () => {
    if (!onWriteField) return;
    if (!batchField) { setBatchError("请选择字段"); return; }
    const rowsToEdit = filteredRows.filter(r => selectedKeys.has(`${r.file_path}::${r.key}`));
    if (rowsToEdit.length === 0) return;
    setBatchApplying(true);
    setBatchError(null);
    const failedKeys: string[] = [];
    for (const row of rowsToEdit) {
      const existing = row.fields.find(f => f.name === batchField)?.value ?? { kind: "Null" as const };
      const newValue = parseFieldValue(batchValue, existing);
      try {
        await onWriteField(sessionId, row.file_path, row.key, [{ kind: "Field", name: batchField }], newValue, existing);
      } catch {
        failedKeys.push(row.key);
      }
    }
    setBatchApplying(false);
    if (failedKeys.length > 0) {
      const preview = failedKeys.length <= 3 ? failedKeys.join(", ") : failedKeys.slice(0, 3).join(", ") + ` 等 ${failedKeys.length} 条`;
      setBatchError(`写入失败: ${preview}`);
    } else {
      const edited = rowsToEdit.length;
      setSelectedKeys(new Set());
      setBatchField("");
      setBatchValue("");
      showBatchSuccess(`已写入 ${edited} 条记录的 ${batchField} 字段`);
    }
  }, [onWriteField, batchField, batchValue, filteredRows, selectedKeys, sessionId, showBatchSuccess]);

  const handleBatchDelete = useCallback(async () => {
    if (!onDeleteRecord) return;
    const rowsToDelete = filteredRows.filter(r => selectedKeys.has(`${r.file_path}::${r.key}`));
    if (rowsToDelete.length === 0) return;
    setBatchApplying(true);
    const failedKeys: string[] = [];
    let deleted = 0;
    for (const row of rowsToDelete) {
      try { await onDeleteRecord(sessionId, row.file_path, row.key); deleted++; }
      catch { failedKeys.push(row.key); }
    }
    setBatchApplying(false);
    setBatchDeletePending(false);
    if (failedKeys.length > 0) {
      const preview = failedKeys.length <= 3 ? failedKeys.join(", ") : failedKeys.slice(0, 3).join(", ") + ` 等 ${failedKeys.length} 条`;
      setBatchError(`删除失败: ${preview}`);
    } else {
      setSelectedKeys(new Set());
      showBatchSuccess(`已删除 ${deleted} 条记录`);
    }
  }, [onDeleteRecord, filteredRows, selectedKeys, sessionId, showBatchSuccess]);

  const handleDuplicateCommit = useCallback(async () => {
    if (!duplicateModal || !onDuplicateRecord) return;
    const newKey = duplicateModal.draft.trim();
    if (!newKey) { setDuplicateModal(m => m && ({ ...m, error: "Key 不能为空" })); return; }
    if (newKey === duplicateModal.srcKey) { setDuplicateModal(null); return; }
    try {
      await onDuplicateRecord(sessionId, duplicateModal.filePath, duplicateModal.srcKey, newKey);
      setDuplicateModal(null);
      onNavigate({ view: "record", file: duplicateModal.filePath, recordKey: newKey });
    } catch (e) { setDuplicateModal(m => m && ({ ...m, error: String(e) })); }
  }, [duplicateModal, onDuplicateRecord, sessionId, onNavigate]);

  const SCALAR_KINDS = ["Null", "Bool", "Int", "Float", "Str", "Enum", "Ref"];

  const handleCellClick = useCallback((e: React.MouseEvent, row: RecordRow, fieldName: string, value: FieldValue) => {
    if (!onWriteField) return;
    if (!SCALAR_KINDS.includes(value.kind)) return;
    if (row.spread_fields.includes(fieldName)) return;
    e.stopPropagation();
    if (value.kind === "Bool") {
      onWriteField(sessionId, row.file_path, row.key, [{ kind: "Field", name: fieldName }], { kind: "Bool", v: !value.v }, value);
      return;
    }
    setEditingCell({ rowKey: row.key, filePath: row.file_path, fieldName, value });
  }, [onWriteField, sessionId]);

  const handleCellCommit = useCallback(async (raw: string) => {
    if (!editingCell || !onWriteField) return;
    const newValue = parseFieldValue(raw, editingCell.value);
    const changed = fieldValueToString(newValue) !== fieldValueToString(editingCell.value) || newValue.kind !== editingCell.value.kind;
    if (!changed) { setEditingCell(null); return; }
    try {
      await onWriteField(sessionId, editingCell.filePath, editingCell.rowKey, [{ kind: "Field", name: editingCell.fieldName }], newValue, editingCell.value);
      setEditingCell(null);
    } catch {
      // onWriteField shows error toast; keep editor open for retry
    }
  }, [editingCell, onWriteField, sessionId]);

  const handleCellTabCommit = useCallback(async (raw: string) => {
    if (!editingCell || !onWriteField) return;
    const newValue = parseFieldValue(raw, editingCell.value);
    const changed = fieldValueToString(newValue) !== fieldValueToString(editingCell.value) || newValue.kind !== editingCell.value.kind;
    if (changed) {
      try { await onWriteField(sessionId, editingCell.filePath, editingCell.rowKey, [{ kind: "Field", name: editingCell.fieldName }], newValue, editingCell.value); }
      catch { /* keep going */ }
    }
    const SCALAR_SET = new Set(["Null", "Bool", "Int", "Float", "Str", "Enum", "Ref"]);
    const findFirstEditable = (r: RecordRow) => {
      for (const name of visibleFieldNames) {
        if (r.spread_fields.includes(name)) continue;
        const v = r.fields.find(f => f.name === name)?.value;
        if (v && SCALAR_SET.has(v.kind) && v.kind !== "Bool") return { name, value: v };
      }
      return null;
    };
    // Find next editable field in same row
    const row = filteredRows.find(r => r.key === editingCell.rowKey && r.file_path === editingCell.filePath);
    if (row) {
      const curIdx = visibleFieldNames.indexOf(editingCell.fieldName);
      for (let i = curIdx + 1; i < visibleFieldNames.length; i++) {
        const nextName = visibleFieldNames[i];
        if (row.spread_fields.includes(nextName)) continue;
        const nextVal = row.fields.find(f => f.name === nextName)?.value;
        if (nextVal && SCALAR_SET.has(nextVal.kind) && nextVal.kind !== "Bool") {
          setEditingCell({ rowKey: row.key, filePath: row.file_path, fieldName: nextName, value: nextVal });
          return;
        }
      }
    }
    // Move to first editable cell of next row
    const rowIdx = filteredRows.findIndex(r => r.key === editingCell.rowKey && r.file_path === editingCell.filePath);
    for (let i = rowIdx + 1; i < filteredRows.length; i++) {
      const nextRow = filteredRows[i];
      const cell = findFirstEditable(nextRow);
      if (cell) {
        setEditingCell({ rowKey: nextRow.key, filePath: nextRow.file_path, fieldName: cell.name, value: cell.value });
        setFocusedIdx(i);
        return;
      }
    }
    setEditingCell(null);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editingCell, onWriteField, sessionId, filteredRows, visibleFieldNames]);

  const handleCellShiftTabCommit = useCallback(async (raw: string) => {
    if (!editingCell || !onWriteField) return;
    const newValue = parseFieldValue(raw, editingCell.value);
    const changed = fieldValueToString(newValue) !== fieldValueToString(editingCell.value) || newValue.kind !== editingCell.value.kind;
    if (changed) {
      try { await onWriteField(sessionId, editingCell.filePath, editingCell.rowKey, [{ kind: "Field", name: editingCell.fieldName }], newValue, editingCell.value); }
      catch { /* keep going */ }
    }
    const SCALAR_SET = new Set(["Null", "Bool", "Int", "Float", "Str", "Enum", "Ref"]);
    const findLastEditable = (r: RecordRow) => {
      for (let i = visibleFieldNames.length - 1; i >= 0; i--) {
        const name = visibleFieldNames[i];
        if (r.spread_fields.includes(name)) continue;
        const v = r.fields.find(f => f.name === name)?.value;
        if (v && SCALAR_SET.has(v.kind) && v.kind !== "Bool") return { name, value: v };
      }
      return null;
    };
    // Find previous editable field in same row
    const row = filteredRows.find(r => r.key === editingCell.rowKey && r.file_path === editingCell.filePath);
    if (row) {
      const curIdx = visibleFieldNames.indexOf(editingCell.fieldName);
      for (let i = curIdx - 1; i >= 0; i--) {
        const prevName = visibleFieldNames[i];
        if (row.spread_fields.includes(prevName)) continue;
        const prevVal = row.fields.find(f => f.name === prevName)?.value;
        if (prevVal && SCALAR_SET.has(prevVal.kind) && prevVal.kind !== "Bool") {
          setEditingCell({ rowKey: row.key, filePath: row.file_path, fieldName: prevName, value: prevVal });
          return;
        }
      }
    }
    // Move to last editable cell of previous row
    const rowIdx = filteredRows.findIndex(r => r.key === editingCell.rowKey && r.file_path === editingCell.filePath);
    for (let i = rowIdx - 1; i >= 0; i--) {
      const prevRow = filteredRows[i];
      const cell = findLastEditable(prevRow);
      if (cell) {
        setEditingCell({ rowKey: prevRow.key, filePath: prevRow.file_path, fieldName: cell.name, value: cell.value });
        setFocusedIdx(i);
        return;
      }
    }
    setEditingCell(null);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editingCell, onWriteField, sessionId, filteredRows, visibleFieldNames]);

  const handlePasteImport = useCallback(async () => {
    if (!pasteModal || !onImportRecord) return;
    if (!pasteModal.source.trim()) { setPasteModal(m => m && ({ ...m, error: "先粘贴 CFD 源码" })); return; }
    const fp = pasteModal.filePath;
    setPasteModal(m => m && ({ ...m, importing: true, error: null }));
    try {
      const importedKeys = await onImportRecord(fp, pasteModal.source);
      if (importedKeys.length === 0) {
        setPasteModal(m => m && ({ ...m, importing: false, error: "未导入任何记录（key 已存在或源码为空）" }));
        return;
      }
      if (importedKeys.length === 1) {
        setPasteModal(null);
        onNavigate({ view: "record", file: fp, recordKey: importedKeys[0] });
      } else {
        setPasteModal(m => m && ({ ...m, importing: false, importedKeys }));
      }
    } catch (e) {
      setPasteModal(m => m && ({ ...m, importing: false, error: String(e) }));
    }
  }, [pasteModal, onImportRecord, onNavigate]);

  const handleCreateCommit = useCallback(async () => {
    if (!createModal || !onCreateRecord) return;
    const key = createModal.key.trim();
    if (!key) { setCreateModal(m => m && ({ ...m, error: "Key 不能为空" })); return; }
    const fp = createModal.filePath;
    if (!fp) { setCreateModal(m => m && ({ ...m, error: "请选择目标文件" })); return; }
    setCreateModal(m => m && ({ ...m, creating: true, error: null }));
    try {
      await onCreateRecord(typeName, fp, key);
      setCreateModal(null);
      onNavigate({ view: "record", file: fp, recordKey: key });
    } catch (e) { setCreateModal(m => m && ({ ...m, creating: false, error: String(e) })); }
  }, [createModal, onCreateRecord, typeName, onNavigate]);

  // Reset selection and editing when type changes; reload column visibility from storage
  useEffect(() => {
    setSelectedKeys(new Set());
    setEditingCell(null);
    setShowColPicker(false);
    setShowRequiredOnly(false);
    try {
      const stored = localStorage.getItem(`cfd-global-col-vis:${typeName}`);
      setHiddenCols(stored ? new Set<string>(JSON.parse(stored)) : new Set<string>());
    } catch { setHiddenCols(new Set<string>()); }
    try {
      const stored = localStorage.getItem(`cfd-global-col-width:${typeName}`);
      setColWidths(stored ? JSON.parse(stored) : {});
    } catch { setColWidths({}); }
  }, [typeName]);

  // Persist hidden columns
  useEffect(() => {
    try { localStorage.setItem(`cfd-global-col-vis:${typeName}`, JSON.stringify([...hiddenCols])); } catch { /* ignore */ }
  }, [typeName, hiddenCols]);

  // If the column being edited becomes hidden, cancel the edit
  useEffect(() => {
    if (editingCell && hiddenCols.has(editingCell.fieldName)) setEditingCell(null);
  }, [hiddenCols, editingCell]);

  // Close column picker on outside click
  useEffect(() => {
    if (!showColPicker) return;
    const handler = (e: MouseEvent) => {
      if (colPickerRef.current && !colPickerRef.current.contains(e.target as Node)) setShowColPicker(false);
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [showColPicker]);

  // Keyboard navigation for the table + Ctrl+F focus search
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
        return;
      }
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      if ((e.ctrlKey || e.metaKey) && e.key === "a" && onWriteField) {
        e.preventDefault();
        setSelectedKeys(new Set(filteredRows.map(r => `${r.file_path}::${r.key}`)));
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "d" && onDuplicateRecord) {
        e.preventDefault();
        const focusedRow = focusedIdx !== null ? filteredRows[focusedIdx] : filteredRows[0];
        if (focusedRow) setDuplicateModal({ srcKey: focusedRow.key, filePath: focusedRow.file_path, draft: `${focusedRow.key}_copy`, error: null });
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "n" && onCreateRecord && availableFiles && availableFiles.length > 0) {
        e.preventDefault();
        setCreateModal({ key: "", filePath: availableFiles[0], creating: false, error: null });
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "r") {
        e.preventDefault();
        setShowRequiredOnly(v => !v);
        return;
      }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setFocusedIdx(i => (i === null ? 0 : Math.min(i + 1, filteredRows.length - 1)));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setFocusedIdx(i => (i === null ? 0 : Math.max(i - 1, 0)));
      } else if (e.key === "Home" && e.ctrlKey) {
        e.preventDefault();
        setFocusedIdx(filteredRows.length > 0 ? 0 : null);
      } else if (e.key === "End" && e.ctrlKey) {
        e.preventDefault();
        setFocusedIdx(filteredRows.length > 0 ? filteredRows.length - 1 : null);
      } else if (e.key === "Enter" && focusedIdx !== null && filteredRows[focusedIdx]) {
        e.preventDefault();
        handleRowClick(filteredRows[focusedIdx]);
      } else if (e.key === "F2" && focusedIdx !== null && onWriteField) {
        const row = filteredRows[focusedIdx];
        if (row) {
          e.preventDefault();
          const SCALAR_KINDS = new Set(["Null", "Int", "Float", "Str", "Enum", "Ref"]);
          const firstEditable = row.fields.find(
            f => !row.spread_fields.includes(f.name) && SCALAR_KINDS.has(f.value.kind)
          );
          if (firstEditable) setEditingCell({ rowKey: row.key, filePath: row.file_path, fieldName: firstEditable.name, value: firstEditable.value });
        }
      } else if ((e.key === "Delete" || e.key === "Backspace") && focusedIdx !== null && onDeleteRecord) {
        const row = filteredRows[focusedIdx];
        if (row) { e.preventDefault(); setSingleDeleteModal({ key: row.key, filePath: row.file_path }); }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [filteredRows, focusedIdx, handleRowClick, onDeleteRecord]);

  // Scroll focused row into view
  useEffect(() => {
    if (focusedIdx !== null) {
      virtualizer.scrollToIndex(focusedIdx, { align: "auto" });
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusedIdx]);

  // Reset focus on type/search change
  useEffect(() => { setFocusedIdx(null); }, [typeName, search]);

  const exportCsv = () => {
    const cols = ["key", "file", ...visibleFieldNames];
    const lines = [cols.join(",")];
    for (const row of filteredRows) {
      const cells = [
        JSON.stringify(row.key),
        JSON.stringify(row.file_path.split(/[\\/]/).pop() ?? row.file_path),
        ...visibleFieldNames.map(f => {
          const v = row.fields.find(x => x.name === f)?.value ?? { kind: "Null" as const };
          const s = fieldValueToString(v);
          return s ? JSON.stringify(s) : "";
        }),
      ];
      lines.push(cells.join(","));
    }
    const blob = new Blob([lines.join("\n")], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${typeName}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: 1, overflow: "hidden" }}>
      {/* Type selector tabs */}
      <div style={{ display: "flex", gap: 2, padding: "6px 8px", borderBottom: "1px solid var(--border)", flexWrap: "wrap", flexShrink: 0 }}>
        {allTypeNames.map(t => (
          <button
            key={t}
            onClick={() => onTypeChange(t)}
            style={{
              padding: "3px 10px",
              fontSize: 12,
              background: t === typeName ? "var(--accent)" : "var(--bg3)",
              border: "none",
              borderRadius: 4,
              color: t === typeName ? "#fff" : "var(--text-muted)",
              cursor: "pointer",
              fontWeight: t === typeName ? 600 : undefined,
            }}
          >
            {t}
            {typeCounts.has(t) && (
              <span style={{ marginLeft: 4, fontSize: 10, opacity: t === typeName ? 0.8 : 0.6, fontWeight: "normal" }}>
                {typeCounts.get(t)}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* Toolbar */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 8px", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
        <input
          ref={searchRef}
          value={search}
          onChange={e => setSearch(e.target.value)}
          onKeyDown={e => {
            if (e.key === "Escape") { setSearch(""); e.stopPropagation(); }
            else if (e.key === "Enter" && filteredRows.length > 0) {
              e.preventDefault();
              const row = filteredRows[focusedIdx ?? 0];
              if (row) onNavigate({ view: "record", file: row.file_path, recordKey: row.key });
            }
          }}
          placeholder="Filter rows… (field:value, file:name, Ctrl+F)"
          style={{
            flex: 1,
            background: "var(--bg3)",
            border: "1px solid var(--border)",
            borderRadius: 4,
            color: "var(--text)",
            padding: "3px 8px",
            fontSize: 12,
            outline: "none",
          }}
        />
        {search && <button onClick={() => setSearch("")} style={{ fontSize: 11, padding: "2px 6px" }}>✕</button>}
        <span style={{ color: "var(--text-muted)", fontSize: 11 }}>
          {filteredRows.length} / {rows.length} records · {new Set(rows.map(r => r.file_path)).size} files
        </span>
        {requiredFieldNames.size > 0 && (
          <button
            onClick={() => setShowRequiredOnly(v => !v)}
            title={showRequiredOnly ? "显示全部记录" : "只显示含空必填字段的记录"}
            style={{
              fontSize: 11,
              padding: "2px 8px",
              background: showRequiredOnly ? "var(--warning)" : "transparent",
              border: `1px solid ${showRequiredOnly ? "var(--warning)" : "var(--border)"}`,
              borderRadius: 4,
              color: showRequiredOnly ? "#000" : "var(--warning)",
              cursor: "pointer",
              whiteSpace: "nowrap",
              fontWeight: showRequiredOnly ? 600 : undefined,
              flexShrink: 0,
            }}
          >
            ⚠ 必填
          </button>
        )}
        {fieldNames.length > 0 && (
          <div ref={colPickerRef} style={{ position: "relative", flexShrink: 0 }}>
            <button
              onClick={() => setShowColPicker(v => !v)}
              title="显示/隐藏列"
              style={{ fontSize: 11, padding: "2px 8px", background: showColPicker ? "var(--bg3)" : "transparent", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text-muted)", cursor: "pointer", whiteSpace: "nowrap" }}
            >
              ⊞ 列
              {hiddenCols.size > 0 && <span style={{ marginLeft: 4, color: "var(--accent)", fontSize: 10 }}>●</span>}
            </button>
            {showColPicker && (
              <div style={{ position: "absolute", top: "100%", right: 0, marginTop: 4, background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 6, padding: "6px 0", zIndex: 1000, minWidth: 160, maxHeight: 320, overflowY: "auto", boxShadow: "0 4px 16px rgba(0,0,0,0.4)" }}>
                <div style={{ padding: "2px 10px 6px", display: "flex", gap: 6 }}>
                  <button onClick={() => setHiddenCols(new Set())} style={{ fontSize: 10, padding: "1px 6px", flex: 1 }}>全显</button>
                  <button onClick={() => setHiddenCols(new Set(fieldNames))} style={{ fontSize: 10, padding: "1px 6px", flex: 1 }}>全隐</button>
                </div>
                {fieldNames.map(f => (
                  <label
                    key={f}
                    style={{ display: "flex", alignItems: "center", gap: 8, padding: "3px 10px", cursor: "pointer", fontSize: 12, color: "var(--text)" }}
                    onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
                    onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
                  >
                    <input
                      type="checkbox"
                      checked={!hiddenCols.has(f)}
                      onChange={e => setHiddenCols(prev => {
                        const next = new Set(prev);
                        if (e.target.checked) next.delete(f); else next.add(f);
                        return next;
                      })}
                      style={{ margin: 0, cursor: "pointer" }}
                    />
                    <span style={{ fontFamily: "monospace" }}>{f}</span>
                  </label>
                ))}
              </div>
            )}
          </div>
        )}
        <button onClick={exportCsv} title="Export as CSV" style={{ fontSize: 11, padding: "2px 8px", background: "transparent", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text-muted)", cursor: "pointer" }}>
          ↓ CSV
        </button>
        {onImportRecord && availableFiles && availableFiles.length > 0 && (
          <button
            onClick={() => setPasteModal({ source: "", filePath: availableFiles[0], importing: false, error: null })}
            title="粘贴 CFD 源码导入记录"
            style={{ fontSize: 11, padding: "2px 8px", background: "transparent", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text-muted)", cursor: "pointer", flexShrink: 0 }}
          >⎘ 粘贴 CFD</button>
        )}
        {onCreateRecord && availableFiles && availableFiles.length > 0 && (
          <button
            onClick={() => setCreateModal({ key: "", filePath: availableFiles[0], creating: false, error: null })}
            title={`新建 ${typeName} 记录 (Ctrl+N)`}
            style={{ fontSize: 11, padding: "2px 8px", background: "transparent", border: "1px solid var(--accent)", borderRadius: 4, color: "var(--accent)", cursor: "pointer", flexShrink: 0 }}
          >＋ New</button>
        )}
      </div>

      {error && (
        <div style={{ padding: 12, color: "var(--error)", fontSize: 13 }}>{error}</div>
      )}
      {loading && (
        <div style={{ padding: 12, color: "var(--text-muted)", fontSize: 13 }}>Loading…</div>
      )}

      {!loading && !error && rows.length === 0 && (
        <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", flexDirection: "column", gap: 8, color: "var(--text-muted)", fontSize: 13 }}>
          <span style={{ fontSize: 28 }}>𝌄</span>
          <span>项目中没有 <strong style={{ color: "var(--text)" }}>{typeName}</strong> 类型的记录</span>
        </div>
      )}

      {!loading && !error && rows.length > 0 && (
        <div style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column" }}>
          {/* Header row */}
          <div style={{ display: "flex", borderBottom: "1px solid var(--border)", background: "var(--bg2)", flexShrink: 0, userSelect: "none" }}>
            {onWriteField && (
              <div style={{ width: 32, flexShrink: 0, display: "flex", alignItems: "center", justifyContent: "center", borderRight: "1px solid var(--border)" }}>
                <input
                  type="checkbox"
                  checked={filteredRows.length > 0 && filteredRows.every(r => selectedKeys.has(`${r.file_path}::${r.key}`))}
                  onChange={e => {
                    if (e.target.checked) setSelectedKeys(new Set(filteredRows.map(r => `${r.file_path}::${r.key}`)));
                    else setSelectedKeys(new Set());
                  }}
                  title="全选/取消全选"
                />
              </div>
            )}
            {(["key", "file", ...visibleFieldNames] as string[]).map((col, i) => {
              const isKey = col === "key";
              const isFile = col === "file";
              const colId = isKey ? "__key__" : isFile ? "__file__" : col;
              const w = isKey ? COL_KEY : isFile ? COL_FILE : getColWidth(col);
              const isSorted = sort?.col === col;
              return (
                <div
                  key={col}
                  onClick={() => handleSortClick(col)}
                  title={(() => {
                    const schema = fieldSchemas.find(s => s.name === col);
                    return schema ? `${col}: ${schema.type_str} — click to sort` : `Sort by ${col}`;
                  })()}
                  style={{
                    width: w,
                    flexShrink: 0,
                    padding: "4px 8px",
                    fontSize: 11,
                    fontWeight: 600,
                    color: isSorted ? "var(--accent)" : "var(--text-muted)",
                    borderRight: "1px solid var(--border)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    cursor: "pointer",
                    display: "flex",
                    alignItems: "center",
                    gap: 3,
                    borderBottom: isSorted ? "2px solid var(--accent)" : undefined,
                    boxSizing: "border-box",
                    position: "relative",
                  }}
                  onMouseEnter={e => { (e.currentTarget as HTMLElement).style.background = "var(--bg3)"; }}
                  onMouseLeave={e => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
                >
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", flex: 1 }}>{col}</span>
                  {isSorted && <span style={{ fontSize: 9, flexShrink: 0 }}>{sort!.dir === "asc" ? "▲" : "▼"}</span>}
                  {!isSorted && i === 0 && <span style={{ fontSize: 9, opacity: 0.3, flexShrink: 0 }}>⇅</span>}
                  <span
                    onMouseDown={e => { e.stopPropagation(); handleColResizeMouseDown(colId, e); }}
                    onClick={e => e.stopPropagation()}
                    style={{ position: "absolute", right: 0, top: 0, width: 6, height: "100%", cursor: "col-resize", zIndex: 1 }}
                  />
                </div>
              );
            })}
          </div>

          {/* Virtualized rows */}
          <div ref={containerRef} style={{ flex: 1, overflowY: "auto", overflowX: "auto" }}>
            <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
              {virtualizer.getVirtualItems().map(vi => {
                const row = filteredRows[vi.index];
                const filename = row.file_path.split(/[\\/]/).pop() ?? row.file_path;
                const isFocused = vi.index === focusedIdx;
                const rowId = `${row.file_path}::${row.key}`;
                const isSelected = selectedKeys.has(rowId);
                return (
                  <div
                    key={rowId}
                    onClick={() => { setFocusedIdx(vi.index); handleRowClick(row); }}
                    onContextMenu={e => handleRowContextMenu(e, row)}
                    style={{
                      position: "absolute",
                      top: vi.start,
                      left: 0,
                      height: ROW_H,
                      display: "flex",
                      alignItems: "center",
                      cursor: "pointer",
                      borderBottom: "1px solid var(--bg3)",
                      width: "max-content",
                      minWidth: "100%",
                      background: isSelected ? "rgba(var(--accent-rgb,98,114,164),0.15)" : isFocused ? "var(--bg3)" : "transparent",
                      borderLeft: isFocused ? "2px solid var(--accent)" : "2px solid transparent",
                    }}
                    onMouseEnter={e => { if (!isFocused && !isSelected) e.currentTarget.style.background = "var(--bg3)"; }}
                    onMouseLeave={e => { if (!isFocused && !isSelected) e.currentTarget.style.background = "transparent"; }}
                  >
                    {onWriteField && (
                      <div style={{ width: 32, flexShrink: 0, display: "flex", alignItems: "center", justifyContent: "center", borderRight: "1px solid var(--border)" }}>
                        <input
                          type="checkbox"
                          checked={isSelected}
                          onChange={e => {
                            e.stopPropagation();
                            setSelectedKeys(prev => {
                              const next = new Set(prev);
                              if (e.target.checked) next.add(rowId); else next.delete(rowId);
                              return next;
                            });
                          }}
                          onClick={e => e.stopPropagation()}
                        />
                      </div>
                    )}
                    <div style={{ width: COL_KEY, flexShrink: 0, padding: "0 8px", fontSize: 12, fontFamily: "monospace", fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", borderRight: "1px solid var(--border)", display: "flex", alignItems: "center", gap: 3 }} title={row.key}>
                      {row.is_fallback && <span style={{ color: "var(--warning)", flexShrink: 0 }} title="Model build failed">⚠</span>}
                      <span style={{ overflow: "hidden", textOverflow: "ellipsis", flex: 1 }}>{row.key}</span>
                      {(() => { const d = rowDiagCounts.get(row.key); if (!d) return null; return (
                        <>
                          {d.errors > 0 && <span style={{ fontSize: 9, background: "var(--error)", color: "#fff", borderRadius: 3, padding: "0 3px", flexShrink: 0 }} title={`${d.errors} error(s)`}>{d.errors}</span>}
                          {d.warnings > 0 && <span style={{ fontSize: 9, background: "var(--warning)", color: "#fff", borderRadius: 3, padding: "0 3px", flexShrink: 0 }} title={`${d.warnings} warning(s)`}>{d.warnings}</span>}
                        </>
                      ); })()}
                    </div>
                    <div style={{ width: COL_FILE, flexShrink: 0, padding: "0 8px", fontSize: 11, fontFamily: "monospace", color: "var(--text-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", borderRight: "1px solid var(--border)" }} title={row.file_path}>
                      {filename}
                    </div>
                    {visibleFieldNames.map(f => {
                      const cell = row.fields.find(x => x.name === f);
                      const isSpread = row.spread_fields.includes(f);
                      const isEditing = editingCell?.rowKey === row.key && editingCell?.filePath === row.file_path && editingCell?.fieldName === f;
                      const isScalar = cell && SCALAR_KINDS.includes(cell.value.kind);
                      const canEdit = !!onWriteField && isScalar && !isSpread;
                      const isRequiredNull = requiredFieldNames.has(f) && (!cell || cell.value.kind === "Null") && !isSpread;
                      return (
                        <div
                          key={f}
                          onClick={cell && canEdit ? (e => handleCellClick(e, row, f, cell.value)) : undefined}
                          onContextMenu={cell ? (e => {
                            const cv = cell.value;
                            const items: { label: string; onClick: () => void }[] = [];
                            let copyText: string | null = null;
                            switch (cv.kind) {
                              case "Null": copyText = "null"; break;
                              case "Bool": copyText = String(cv.v); break;
                              case "Int": case "Float": copyText = String(cv.v); break;
                              case "Str": copyText = cv.v; break;
                              case "Enum": copyText = cv.variant; break;
                              case "Ref": copyText = cv.target_key; break;
                            }
                            if (copyText !== null) {
                              const text = copyText;
                              items.push({ label: "复制值", onClick: () => navigator.clipboard.writeText(text).then(() => onSuccess?.(`已复制: ${text}`)).catch(err => onError?.(`复制失败: ${err}`)) });
                            }
                            if (cv.kind === "Ref") {
                              const refValue = cv;
                              items.push({ label: "跳转到引用记录", onClick: () => onNavigate({ view: "record", file: refValue.target_file ?? row.file_path, recordKey: refValue.target_key }) });
                            }
                            if (items.length > 0) {
                              e.preventDefault();
                              e.stopPropagation();
                              setContextMenu({ x: e.clientX, y: e.clientY, items });
                            }
                          }) : undefined}
                          style={{
                            width: getColWidth(f),
                            flexShrink: 0,
                            padding: isEditing ? 0 : "0 4px",
                            overflow: "hidden",
                            borderRight: "1px solid var(--border)",
                            height: "100%",
                            display: "flex",
                            alignItems: "center",
                            opacity: isSpread ? 0.55 : 1,
                            cursor: canEdit ? "text" : "default",
                            color: isRequiredNull ? "var(--warning)" : undefined,
                          }}
                          title={isSpread ? `${f} (inherited via spread — edit in source record)` : isRequiredNull ? `${f} — required field (no default)` : canEdit ? `Click to edit ${f}` : undefined}
                        >
                          {isEditing ? (
                            <CellEditor
                              value={editingCell!.value}
                              sessionId={sessionId}
                              onCommit={handleCellCommit}
                              onCancel={() => setEditingCell(null)}
                              onTabCommit={handleCellTabCommit}
                              onShiftTabCommit={handleCellShiftTabCommit}
                            />
                          ) : cell ? (
                            <DataCard
                              value={cell.value}
                              mode="compact"
                              depth={0}
                              sessionId={sessionId}
                              onRefClick={(targetFile, targetKey) =>
                                onNavigate({ view: "record", file: targetFile ?? row.file_path, recordKey: targetKey })
                              }
                            />
                          ) : (
                            <span style={{ color: "var(--text-muted)", fontSize: 11 }}>—</span>
                          )}
                        </div>
                      );
                    })}
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}

      {/* Batch-edit bar — visible when rows are selected */}
      {selectedKeys.size > 0 && (
        <div style={{ borderTop: "1px solid var(--border)", padding: "6px 8px", flexShrink: 0, display: "flex", alignItems: "center", gap: 8, background: "var(--bg2)" }}>
          <span style={{ fontSize: 11, color: "var(--text-muted)", flexShrink: 0 }}>
            已选 {selectedKeys.size} 条
          </span>
          {onWriteField && (<>
            <input
              value={batchField}
              onChange={e => { setBatchField(e.target.value); setBatchError(null); }}
              placeholder="字段名"
              list="global-batch-fields"
              style={{ width: 120, background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "2px 6px", fontSize: 12, outline: "none" }}
            />
            <datalist id="global-batch-fields">
              {fieldNames.map(f => <option key={f} value={f} />)}
            </datalist>
            <span style={{ fontSize: 11, color: "var(--text-muted)" }}>→</span>
            <input
              value={batchValue}
              onChange={e => setBatchValue(e.target.value)}
              placeholder="新值"
              style={{ flex: 1, minWidth: 80, background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "2px 6px", fontSize: 12, outline: "none" }}
              onKeyDown={e => { if (e.key === "Enter" && !batchApplying && batchField) handleBatchApply(); }}
            />
            <button
              className="primary"
              onClick={handleBatchApply}
              disabled={batchApplying || !batchField}
              style={{ fontSize: 11, padding: "2px 10px", flexShrink: 0 }}
            >
              {batchApplying ? "写入中…" : "批量写入"}
            </button>
          </>)}
          {batchError && <span style={{ color: "#ff5555", fontSize: 11 }}>{batchError}</span>}
          {batchSuccess && !batchError && <span style={{ color: "var(--accent)", fontSize: 11 }}>✓ {batchSuccess}</span>}
          {onDeleteRecord && (
            <button
              onClick={() => setBatchDeletePending(true)}
              disabled={batchApplying}
              style={{ fontSize: 11, padding: "2px 8px", flexShrink: 0, color: "var(--error)", border: "1px solid var(--error)", borderRadius: 4, background: "transparent", cursor: "pointer" }}
            >
              批量删除
            </button>
          )}
          <button
            onClick={() => {
              const selected = filteredRows.filter(r => selectedKeys.has(`${r.file_path}::${r.key}`));
              if (selected.length === 0) return;
              const header = ["key", "file", ...visibleFieldNames].join(",");
              function csvEscape(s: string): string {
                if (s.includes(",") || s.includes('"') || s.includes("\n")) return `"${s.replace(/"/g, '""')}"`;
                return s;
              }
              function cellText(v: FieldValue | undefined): string {
                if (!v) return "";
                switch (v.kind) {
                  case "Null": return "";
                  case "Bool": return String(v.v);
                  case "Int": case "Float": return String(v.v);
                  case "Str": return v.v;
                  case "Enum": return v.variant;
                  case "Ref": return v.target_key;
                  default: return "";
                }
              }
              const lines = selected.map(r => {
                const cells = [r.key, r.file_path.split(/[\\/]/).pop() ?? r.file_path, ...visibleFieldNames.map(f => r.fields.find(x => x.name === f)?.value)].map((v, i) => {
                  return csvEscape(typeof v === "string" ? v : cellText(v as FieldValue | undefined));
                });
                return cells.join(",");
              });
              navigator.clipboard.writeText([header, ...lines].join("\n")).then(() => onSuccess?.(`已复制 ${selected.length} 行为 CSV`)).catch(e => onError?.(`复制失败: ${e}`));
            }}
            title="复制选中行为 CSV"
            style={{ fontSize: 11, padding: "2px 8px", flexShrink: 0 }}
          >⎘ CSV</button>
          <button
            onClick={() => {
              const selected = filteredRows.filter(r => selectedKeys.has(`${r.file_path}::${r.key}`));
              if (selected.length === 0) return;
              const arr = selected.map(r => {
                const obj: Record<string, unknown> = { _key: r.key, _file: r.file_path.split(/[\\/]/).pop() ?? r.file_path };
                for (const f of r.fields) obj[f.name] = fieldValueToJson(f.value);
                return obj;
              });
              navigator.clipboard.writeText(JSON.stringify(arr, null, 2)).then(() => onSuccess?.(`已复制 ${arr.length} 行为 JSON`)).catch(e => onError?.(`复制失败: ${e}`));
            }}
            title="复制选中行为 JSON 数组"
            style={{ fontSize: 11, padding: "2px 8px", flexShrink: 0 }}
          >⎘ JSON</button>
          <button onClick={() => setSelectedKeys(new Set())} style={{ fontSize: 11, padding: "2px 8px", flexShrink: 0 }}>取消选择</button>
        </div>
      )}

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenu.items}
          onClose={() => setContextMenu(null)}
        />
      )}

      {/* Duplicate record modal */}
      {duplicateModal && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 3000 }}
          onClick={() => setDuplicateModal(null)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, minWidth: 340, boxShadow: "0 8px 32px rgba(0,0,0,0.5)" }}
            onClick={e => e.stopPropagation()}
          >
            <div style={{ fontWeight: 600, marginBottom: 4 }}>复制记录</div>
            <div style={{ color: "var(--text-muted)", fontSize: 12, marginBottom: 12 }}>
              从 <code style={{ fontFamily: "monospace" }}>{duplicateModal.srcKey}</code> 复制，输入新 Key：
            </div>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 4 }}>
              <span style={{ fontSize: 12, color: "var(--text-muted)" }}>新 Key</span>
              <button
                type="button"
                onClick={() => {
                  const base = duplicateModal.srcKey;
                  const existingKeys = new Set(rows.map(r => r.key));
                  let n = 1;
                  while (existingKeys.has(`${base}_copy_${String(n).padStart(3, "0")}`)) n++;
                  setDuplicateModal(m => m && ({ ...m, draft: `${base}_copy_${String(n).padStart(3, "0")}`, error: null }));
                }}
                style={{ fontSize: 11, padding: "1px 6px", background: "transparent", border: "1px solid var(--border)", borderRadius: 3, color: "var(--text-muted)", cursor: "pointer" }}
              >
                ✦ 建议
              </button>
            </div>
            <input
              value={duplicateModal.draft}
              onChange={e => setDuplicateModal(m => m && ({ ...m, draft: e.target.value, error: null }))}
              onKeyDown={e => {
                if (e.key === "Enter") { e.preventDefault(); handleDuplicateCommit(); }
                if (e.key === "Escape") setDuplicateModal(null);
                e.stopPropagation();
              }}
              // eslint-disable-next-line jsx-a11y/no-autofocus
              autoFocus
              style={{ width: "100%", background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "4px 8px", fontSize: 13, outline: "none", fontFamily: "monospace", boxSizing: "border-box" }}
            />
            {duplicateModal.error && <div style={{ color: "var(--error)", fontSize: 12, marginTop: 4 }}>{duplicateModal.error}</div>}
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 16 }}>
              <button onClick={() => setDuplicateModal(null)}>取消</button>
              <button className="primary" onClick={handleDuplicateCommit} disabled={!duplicateModal.draft.trim()}>复制</button>
            </div>
          </div>
        </div>
      )}

      {/* Create record modal */}
      {createModal && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 3000 }}
          onClick={() => !createModal.creating && setCreateModal(null)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, minWidth: 360, boxShadow: "0 8px 32px rgba(0,0,0,0.5)" }}
            onClick={e => e.stopPropagation()}
          >
            <div style={{ fontWeight: 600, marginBottom: 4 }}>新建 {typeName} 记录</div>
            <div style={{ display: "flex", flexDirection: "column", gap: 10, marginTop: 12 }}>
              <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12, color: "var(--text-muted)" }}>
                <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                  Key
                  <button
                    type="button"
                    onClick={() => {
                      const prefix = typeName.replace(/([A-Z])/g, m => `_${m.toLowerCase()}`).replace(/^_/, "").toLowerCase();
                      const existingKeys = new Set(rows.map(r => r.key));
                      let n = 1;
                      while (existingKeys.has(`${prefix}_${String(n).padStart(3, "0")}`)) n++;
                      setCreateModal(m => m && ({ ...m, key: `${prefix}_${String(n).padStart(3, "0")}`, error: null }));
                    }}
                    style={{ fontSize: 10, padding: "1px 6px", background: "transparent", border: "1px solid var(--border)", borderRadius: 3, color: "var(--text-muted)", cursor: "pointer" }}
                  >
                    ✦ 建议
                  </button>
                </div>
                <input
                  value={createModal.key}
                  onChange={e => setCreateModal(m => m && ({ ...m, key: e.target.value, error: null }))}
                  onKeyDown={e => {
                    if (e.key === "Enter") { e.preventDefault(); handleCreateCommit(); }
                    if (e.key === "Escape") setCreateModal(null);
                    e.stopPropagation();
                  }}
                  placeholder="record_key"
                  // eslint-disable-next-line jsx-a11y/no-autofocus
                  autoFocus
                  style={{ background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "4px 8px", fontSize: 13, outline: "none", fontFamily: "monospace" }}
                />
              </label>
              {availableFiles && availableFiles.length > 1 && (
                <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12, color: "var(--text-muted)" }}>
                  目标文件
                  <select
                    value={createModal.filePath}
                    onChange={e => setCreateModal(m => m && ({ ...m, filePath: e.target.value, error: null }))}
                    style={{ background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "4px 8px", fontSize: 13, outline: "none", fontFamily: "monospace" }}
                  >
                    {availableFiles.map(f => (
                      <option key={f} value={f}>{f.split(/[\\/]/).pop() ?? f}</option>
                    ))}
                  </select>
                </label>
              )}
              {createModal.error && <div style={{ color: "var(--error)", fontSize: 12 }}>{createModal.error}</div>}
            </div>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 16 }}>
              <button onClick={() => setCreateModal(null)} disabled={createModal.creating}>取消</button>
              <button className="primary" onClick={handleCreateCommit} disabled={createModal.creating || !createModal.key.trim()}>
                {createModal.creating ? "创建中…" : "创建"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Batch delete confirmation */}
      {batchDeletePending && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 3000 }}
          onClick={() => setBatchDeletePending(false)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, minWidth: 320, boxShadow: "0 8px 32px rgba(0,0,0,0.5)" }}
            onClick={e => e.stopPropagation()}
          >
            <div style={{ fontWeight: 600, marginBottom: 8 }}>确认批量删除</div>
            <div style={{ color: "var(--text-muted)", fontSize: 13, marginBottom: 16 }}>
              即将删除 <strong style={{ color: "var(--error)" }}>{selectedKeys.size}</strong> 条记录，此操作不可撤销。
            </div>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setBatchDeletePending(false)}>取消</button>
              <button
                onClick={handleBatchDelete}
                disabled={batchApplying}
                style={{ background: "var(--error)", color: "#fff", border: "none", borderRadius: 4, padding: "4px 16px", cursor: "pointer", fontWeight: 600 }}
              >
                {batchApplying ? "删除中…" : `删除 ${selectedKeys.size} 条`}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Single record delete confirmation (triggered by Delete key) */}
      {singleDeleteModal && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 3000 }}
          onClick={() => setSingleDeleteModal(null)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, minWidth: 320, boxShadow: "0 8px 32px rgba(0,0,0,0.5)" }}
            onClick={e => e.stopPropagation()}
            onKeyDown={e => { if (e.key === "Escape") setSingleDeleteModal(null); e.stopPropagation(); }}
          >
            <div style={{ fontWeight: 600, marginBottom: 8 }}>确认删除记录</div>
            <div style={{ color: "var(--text-muted)", fontSize: 13, marginBottom: 16 }}>
              即将删除 <strong style={{ color: "var(--error)", fontFamily: "monospace" }}>{singleDeleteModal.key}</strong>，此操作不可撤销。
            </div>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setSingleDeleteModal(null)}>取消</button>
              <button
                autoFocus
                onClick={() => {
                  if (!onDeleteRecord) return;
                  onDeleteRecord(sessionId, singleDeleteModal.filePath, singleDeleteModal.key)
                    .catch(err => onError?.(`删除失败: ${err}`));
                  setSingleDeleteModal(null);
                }}
                style={{ background: "var(--error)", color: "#fff", border: "none", borderRadius: 4, padding: "4px 16px", cursor: "pointer", fontWeight: 600 }}
              >
                删除
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Paste CFD import modal */}
      {pasteModal && (
        <div
          style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 3000 }}
          onClick={() => !pasteModal.importing && setPasteModal(null)}
        >
          <div
            style={{ background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: 24, minWidth: 400, maxWidth: 560, boxShadow: "0 8px 32px rgba(0,0,0,0.5)", display: "flex", flexDirection: "column", gap: 12 }}
            onClick={e => e.stopPropagation()}
          >
            <h3 style={{ margin: 0, fontSize: 15 }}>粘贴 CFD 源码导入</h3>
            {pasteModal.importedKeys ? (
              <>
                <div style={{ color: "var(--success, #50fa7b)", fontSize: 13, background: "rgba(80,250,123,0.08)", border: "1px solid rgba(80,250,123,0.3)", borderRadius: 4, padding: "8px 12px" }}>
                  ✓ 已导入 {pasteModal.importedKeys.length} 条记录
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 4, maxHeight: 160, overflowY: "auto" }}>
                  {pasteModal.importedKeys.map(k => (
                    <button
                      key={k}
                      onClick={() => { setPasteModal(null); onNavigate({ view: "record", file: pasteModal.filePath, recordKey: k }); }}
                      style={{ textAlign: "left", fontFamily: "monospace", fontSize: 12, padding: "2px 8px", background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--accent)", cursor: "pointer" }}
                    >{k}</button>
                  ))}
                </div>
                <div style={{ display: "flex", justifyContent: "flex-end" }}>
                  <button onClick={() => setPasteModal(null)}>关闭</button>
                </div>
              </>
            ) : (
              <>
                {availableFiles && availableFiles.length > 1 && (
                  <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12, color: "var(--text-muted)" }}>
                    目标文件
                    <select
                      value={pasteModal.filePath}
                      onChange={e => setPasteModal(m => m && ({ ...m, filePath: e.target.value }))}
                      style={{ background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "4px 8px", fontSize: 13, outline: "none", fontFamily: "monospace" }}
                    >
                      {availableFiles.map(f => <option key={f} value={f}>{f.split(/[\\/]/).pop() ?? f}</option>)}
                    </select>
                  </label>
                )}
                <textarea
                  value={pasteModal.source}
                  onChange={e => setPasteModal(m => m && ({ ...m, source: e.target.value, error: null }))}
                  onKeyDown={e => {
                    if ((e.ctrlKey || e.metaKey) && e.key === "Enter" && !pasteModal.importing && pasteModal.source.trim()) {
                      e.preventDefault();
                      handlePasteImport();
                    }
                    if (e.key === "Escape") { e.preventDefault(); setPasteModal(null); }
                    e.stopPropagation();
                  }}
                  placeholder="粘贴 CFD 源码…"
                  // eslint-disable-next-line jsx-a11y/no-autofocus
                  autoFocus
                  rows={8}
                  style={{ fontFamily: "monospace", fontSize: 12, background: "var(--bg3)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "6px 8px", resize: "vertical", outline: "none" }}
                />
                {pasteModal.error && <div style={{ color: "var(--error)", fontSize: 12 }}>{pasteModal.error}</div>}
                <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
                  <button onClick={() => setPasteModal(null)} disabled={pasteModal.importing}>取消</button>
                  <button
                    className="primary"
                    onClick={handlePasteImport}
                    disabled={pasteModal.importing || !pasteModal.source.trim()}
                  >
                    {pasteModal.importing ? "导入中…" : "导入"}
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
