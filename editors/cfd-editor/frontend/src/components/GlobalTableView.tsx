import { useState, useEffect, useRef, useCallback } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { RecordRow, FieldValue } from "../bindings";
import type { Route } from "../router";
import { DataCard } from "./DataCard";
import { api } from "../api";

interface GlobalTableViewProps {
  sessionId: number;
  typeName: string;
  onTypeChange: (typeName: string) => void;
  onNavigate: (route: Route) => void;
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

export function GlobalTableView({ sessionId, typeName, onTypeChange, onNavigate }: GlobalTableViewProps) {
  const [rows, setRows] = useState<RecordRow[]>([]);
  const [allTypeNames, setAllTypeNames] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortCol | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

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
  }, [sessionId, typeName]);

  const filteredRows = (() => {
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
  })();

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
          value={search}
          onChange={e => setSearch(e.target.value)}
          placeholder="Filter rows…"
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
                return (
                  <div
                    key={`${row.file_path}::${row.key}`}
                    onClick={() => handleRowClick(row)}
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
                    }}
                    onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
                    onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
                  >
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
    </div>
  );
}
