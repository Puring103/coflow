import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { RecordRow, FieldValue, FieldPathSegment, DiagnosticItem } from "../bindings";
import type { Route } from "../router";
import { DataCard } from "./DataCard";
import { ContextMenu, type ContextMenuState } from "./ContextMenu";
import { api } from "../api";

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
  availableFiles?: string[];
  diagnostics?: DiagnosticItem[];
  onError?: (msg: string) => void;
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

export function GlobalTableView({ sessionId, typeName, refreshKey, onTypeChange, onNavigate, onWriteField, onDeleteRecord, onDuplicateRecord, onMoveRecord, onCopyRecord, onCreateRecord, availableFiles, diagnostics, onError }: GlobalTableViewProps) {
  const [rows, setRows] = useState<RecordRow[]>([]);
  const [allTypeNames, setAllTypeNames] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortCol | null>(null);
  const [focusedIdx, setFocusedIdx] = useState<number | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [batchField, setBatchField] = useState("");
  const [batchValue, setBatchValue] = useState("");
  const [batchApplying, setBatchApplying] = useState(false);
  const [batchError, setBatchError] = useState<string | null>(null);
  const [batchDeletePending, setBatchDeletePending] = useState(false);
  const [duplicateModal, setDuplicateModal] = useState<{ srcKey: string; filePath: string; draft: string; error: string | null } | null>(null);
  const [typeCounts, setTypeCounts] = useState<Map<string, number>>(new Map());
  const [createModal, setCreateModal] = useState<{ key: string; filePath: string; creating: boolean; error: string | null } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

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
    if (!typeName) return;
    setLoading(true);
    setError(null);
    setSort(null);
    api.getAllRecordsOfType(sessionId, typeName)
      .then(r => { setRows(r); setLoading(false); })
      .catch(e => { setError(String(e)); setLoading(false); });
  // refreshKey is intentionally included to allow callers to force a re-fetch
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, typeName, refreshKey]);

  const filteredRows = useMemo(() => {
    const base = rows.filter(r => {
      if (!search) return true;
      const q = search.toLowerCase();
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
  }, [rows, search, sort]);

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
      if (s?.col === col) return s.dir === "asc" ? { col, dir: "desc" } : null;
      return { col, dir: "asc" };
    });
  };

  // Determine field names from union of all records
  const fieldNames: string[] = (() => {
    const seen = new Set<string>();
    const names: string[] = [];
    for (const r of rows) {
      for (const f of r.fields) {
        if (!seen.has(f.name)) { seen.add(f.name); names.push(f.name); }
      }
    }
    return names;
  })();

  const COL_KEY = 120;
  const COL_FILE = 140;
  const COL_FIELD = 120;
  const ROW_H = 36;

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
        { label: "复制 Key", onClick: () => navigator.clipboard.writeText(row.key).catch(e => onError?.(`复制失败: ${e}`)) },
        { label: "复制为 CFD 源码", onClick: () => api.getRecordSource(sessionId, row.file_path, row.key).then(src => navigator.clipboard.writeText(src)).catch(e => onError?.(`复制失败: ${e}`)) },
        ...(onDuplicateRecord ? [{ label: "复制记录 (Ctrl+D)", onClick: () => setDuplicateModal({ srcKey: row.key, filePath: row.file_path, draft: `${row.key}_copy`, error: null }) }] : []),
        ...(onMoveRecord ? [{ label: "移动到文件…", onClick: () => onMoveRecord(row.file_path, row.key) }] : []),
        ...(onCopyRecord ? [{ label: "复制到文件…", onClick: () => onCopyRecord(row.file_path, row.key) }] : []),
        ...(onDeleteRecord ? [{ label: "删除记录", danger: true as const, onClick: () => onDeleteRecord(sessionId, row.file_path, row.key).catch(e => onError?.(`删除失败: ${e}`)) }] : []),
      ],
    });
  }, [sessionId, onNavigate, onDeleteRecord, onMoveRecord, onCopyRecord, onError]);

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
      setSelectedKeys(new Set());
      setBatchField("");
      setBatchValue("");
    }
  }, [onWriteField, batchField, batchValue, filteredRows, selectedKeys, sessionId]);

  const handleBatchDelete = useCallback(async () => {
    if (!onDeleteRecord) return;
    const rowsToDelete = filteredRows.filter(r => selectedKeys.has(`${r.file_path}::${r.key}`));
    if (rowsToDelete.length === 0) return;
    setBatchApplying(true);
    const failedKeys: string[] = [];
    for (const row of rowsToDelete) {
      try { await onDeleteRecord(sessionId, row.file_path, row.key); }
      catch { failedKeys.push(row.key); }
    }
    setBatchApplying(false);
    setBatchDeletePending(false);
    if (failedKeys.length > 0) {
      const preview = failedKeys.length <= 3 ? failedKeys.join(", ") : failedKeys.slice(0, 3).join(", ") + ` 等 ${failedKeys.length} 条`;
      setBatchError(`删除失败: ${preview}`);
    } else {
      setSelectedKeys(new Set());
    }
  }, [onDeleteRecord, filteredRows, selectedKeys, sessionId]);

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

  // Reset selection when type/search changes
  useEffect(() => { setSelectedKeys(new Set()); }, [typeName]);

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
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setFocusedIdx(i => (i === null ? 0 : Math.min(i + 1, filteredRows.length - 1)));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setFocusedIdx(i => (i === null ? 0 : Math.max(i - 1, 0)));
      } else if (e.key === "Enter" && focusedIdx !== null && filteredRows[focusedIdx]) {
        e.preventDefault();
        handleRowClick(filteredRows[focusedIdx]);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [filteredRows, focusedIdx, handleRowClick]);

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
    const cols = ["key", "file", ...fieldNames];
    const lines = [cols.join(",")];
    for (const row of filteredRows) {
      const cells = [
        JSON.stringify(row.key),
        JSON.stringify(row.file_path.split(/[\\/]/).pop() ?? row.file_path),
        ...fieldNames.map(f => {
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
          onKeyDown={e => { if (e.key === "Escape") { setSearch(""); e.stopPropagation(); } }}
          placeholder="Filter rows… (Ctrl+F)"
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
        <button onClick={exportCsv} title="Export as CSV" style={{ fontSize: 11, padding: "2px 8px", background: "transparent", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text-muted)", cursor: "pointer" }}>
          ↓ CSV
        </button>
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
            {(["key", "file", ...fieldNames] as string[]).map((col, i) => {
              const isKey = col === "key";
              const isFile = col === "file";
              const w = isKey ? COL_KEY : isFile ? COL_FILE : COL_FIELD;
              const isSorted = sort?.col === col;
              return (
                <div
                  key={col}
                  onClick={() => handleSortClick(col)}
                  title={`Sort by ${col}`}
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
                  }}
                  onMouseEnter={e => { (e.currentTarget as HTMLElement).style.background = "var(--bg3)"; }}
                  onMouseLeave={e => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
                >
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{col}</span>
                  {isSorted && <span style={{ fontSize: 9, flexShrink: 0 }}>{sort!.dir === "asc" ? "▲" : "▼"}</span>}
                  {!isSorted && i === 0 && <span style={{ fontSize: 9, opacity: 0.3, flexShrink: 0 }}>⇅</span>}
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
                    {fieldNames.map(f => {
                      const cell = row.fields.find(x => x.name === f);
                      const isSpread = row.spread_fields.includes(f);
                      return (
                        <div key={f} style={{ width: COL_FIELD, flexShrink: 0, padding: "0 4px", overflow: "hidden", borderRight: "1px solid var(--border)", height: "100%", display: "flex", alignItems: "center", opacity: isSpread ? 0.55 : 1 }} title={isSpread ? `${f} (inherited via spread — edit in source record)` : undefined}>
                          {cell ? (
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
          {onDeleteRecord && (
            <button
              onClick={() => setBatchDeletePending(true)}
              disabled={batchApplying}
              style={{ fontSize: 11, padding: "2px 8px", flexShrink: 0, color: "var(--error)", border: "1px solid var(--error)", borderRadius: 4, background: "transparent", cursor: "pointer" }}
            >
              批量删除
            </button>
          )}
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
                Key
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
    </div>
  );
}
