import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { RecordRow, FieldValue, FieldPathSegment } from "../bindings";
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
  onMoveRecord?: (srcFile: string, recordKey: string) => void;
  onCopyRecord?: (srcFile: string, recordKey: string) => void;
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

export function GlobalTableView({ sessionId, typeName, refreshKey, onTypeChange, onNavigate, onWriteField, onDeleteRecord, onMoveRecord, onCopyRecord }: GlobalTableViewProps) {
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
  const containerRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api.getAllTypeNames(sessionId).then(names => setAllTypeNames(names)).catch(() => {});
  }, [sessionId]);

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
        { label: "在资源管理器中显示", onClick: () => api.revealInExplorer(sessionId, row.file_path).catch(() => {}) },
        { label: "复制 Key", onClick: () => navigator.clipboard.writeText(row.key).catch(() => {}) },
        { label: "复制为 CFD 源码", onClick: () => api.getRecordSource(sessionId, row.file_path, row.key).then(src => navigator.clipboard.writeText(src)).catch(() => {}) },
        ...(onMoveRecord ? [{ label: "移动到文件…", onClick: () => onMoveRecord(row.file_path, row.key) }] : []),
        ...(onCopyRecord ? [{ label: "复制到文件…", onClick: () => onCopyRecord(row.file_path, row.key) }] : []),
        ...(onDeleteRecord ? [{ label: "删除记录", danger: true as const, onClick: () => onDeleteRecord(sessionId, row.file_path, row.key).catch(() => {}) }] : []),
      ],
    });
  }, [sessionId, onNavigate, onDeleteRecord, onMoveRecord, onCopyRecord]);

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
      </div>

      {error && (
        <div style={{ padding: 12, color: "var(--error)", fontSize: 13 }}>{error}</div>
      )}
      {loading && (
        <div style={{ padding: 12, color: "var(--text-muted)", fontSize: 13 }}>Loading…</div>
      )}

      {!loading && !error && (
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
                    <div style={{ width: COL_KEY, flexShrink: 0, padding: "0 8px", fontSize: 12, fontFamily: "monospace", fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", borderRight: "1px solid var(--border)" }} title={row.key}>
                      {row.is_fallback && <span style={{ color: "var(--warning)", marginRight: 4 }}>⚠</span>}
                      {row.key}
                    </div>
                    <div style={{ width: COL_FILE, flexShrink: 0, padding: "0 8px", fontSize: 11, fontFamily: "monospace", color: "var(--text-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", borderRight: "1px solid var(--border)" }} title={row.file_path}>
                      {filename}
                    </div>
                    {fieldNames.map(f => {
                      const cell = row.fields.find(x => x.name === f);
                      return (
                        <div key={f} style={{ width: COL_FIELD, flexShrink: 0, padding: "0 4px", overflow: "hidden", borderRight: "1px solid var(--border)", height: "100%", display: "flex", alignItems: "center" }}>
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

      {/* Batch-edit bar — only visible when onWriteField is provided and rows are selected */}
      {onWriteField && selectedKeys.size > 0 && (
        <div style={{ borderTop: "1px solid var(--border)", padding: "6px 8px", flexShrink: 0, display: "flex", alignItems: "center", gap: 8, background: "var(--bg2)" }}>
          <span style={{ fontSize: 11, color: "var(--text-muted)", flexShrink: 0 }}>
            已选 {selectedKeys.size} 条
          </span>
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
          {batchError && <span style={{ color: "#ff5555", fontSize: 11 }}>{batchError}</span>}
          <button
            className="primary"
            onClick={handleBatchApply}
            disabled={batchApplying || !batchField}
            style={{ fontSize: 11, padding: "2px 10px", flexShrink: 0 }}
          >
            {batchApplying ? "写入中…" : "批量写入"}
          </button>
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
    </div>
  );
}
