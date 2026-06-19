import { useState, useEffect, useRef, useCallback } from "react";
import type { RecordBrief } from "../bindings";

interface CommandPaletteProps {
  records: RecordBrief[];
  onNavigate: (filePath: string, recordKey: string) => void;
  onClose: () => void;
}

function highlight(text: string, query: string): React.ReactNode {
  if (!query) return text;
  const idx = text.toLowerCase().indexOf(query.toLowerCase());
  if (idx === -1) return text;
  return (
    <>
      {text.slice(0, idx)}
      <mark style={{ background: "var(--accent)", color: "#fff", borderRadius: 2, padding: "0 1px" }}>
        {text.slice(idx, idx + query.length)}
      </mark>
      {text.slice(idx + query.length)}
    </>
  );
}

function scoreRecord(record: RecordBrief, q: string): number {
  if (!q) return 0;
  const key = record.key.toLowerCase();
  const type = record.actual_type.toLowerCase();
  const file = record.file_path.toLowerCase();
  const query = q.toLowerCase();
  // Exact prefix match on key is best
  if (key.startsWith(query)) return 3;
  if (key.includes(query)) return 2;
  if (type.includes(query)) return 1;
  if (file.includes(query)) return 0;
  return -1;
}

export function CommandPalette({ records, onNavigate, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIdx, setSelectedIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const filtered = query
    ? records
        .map(r => ({ r, score: scoreRecord(r, query) }))
        .filter(({ score }) => score >= 0)
        .sort((a, b) => b.score - a.score || a.r.key.localeCompare(b.r.key))
        .map(({ r }) => r)
    : records.slice(0, 100);

  const clampedIdx = Math.min(selectedIdx, filtered.length - 1);

  const commit = useCallback(() => {
    const item = filtered[clampedIdx];
    if (item) {
      onNavigate(item.file_path, item.key);
      onClose();
    }
  }, [filtered, clampedIdx, onNavigate, onClose]);

  useEffect(() => {
    setSelectedIdx(0);
  }, [query]);

  // Scroll selected item into view
  useEffect(() => {
    const el = listRef.current?.children[clampedIdx] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest" });
  }, [clampedIdx]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIdx(i => Math.min(i + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIdx(i => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") {
      onClose();
    }
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.55)",
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        zIndex: 3000,
        paddingTop: "15vh",
      }}
      onClick={onClose}
    >
      <div
        style={{
          background: "var(--bg2)",
          border: "1px solid var(--border)",
          borderRadius: 10,
          width: 540,
          maxWidth: "90vw",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          boxShadow: "0 8px 40px rgba(0,0,0,0.5)",
        }}
        onClick={e => e.stopPropagation()}
      >
        {/* Search input */}
        <div style={{
          display: "flex",
          alignItems: "center",
          padding: "10px 14px",
          gap: 8,
          borderBottom: "1px solid var(--border)",
        }}>
          <span style={{ color: "var(--text-muted)", fontSize: 14 }}>⌕</span>
          <input
            ref={inputRef}
            value={query}
            onChange={e => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Jump to record… (type key, type, or file)"
            style={{
              flex: 1,
              background: "transparent",
              border: "none",
              outline: "none",
              color: "var(--text)",
              fontSize: 14,
            }}
          />
          {query && (
            <button
              onClick={() => setQuery("")}
              style={{ background: "none", border: "none", color: "var(--text-muted)", cursor: "pointer", fontSize: 13, padding: "2px 4px" }}
            >✕</button>
          )}
          <span style={{ color: "var(--text-muted)", fontSize: 11, flexShrink: 0 }}>Esc to close</span>
        </div>

        {/* Results list */}
        <div
          ref={listRef}
          style={{
            maxHeight: 360,
            overflowY: "auto",
            padding: "4px 0",
          }}
        >
          {filtered.length === 0 ? (
            <div style={{ padding: "12px 14px", color: "var(--text-muted)", fontSize: 13 }}>
              {query ? `No records match "${query}"` : "No records in project"}
            </div>
          ) : (
            filtered.map((r, idx) => {
              const isSelected = idx === clampedIdx;
              return (
                <div
                  key={`${r.file_path}:${r.key}`}
                  onClick={() => { onNavigate(r.file_path, r.key); onClose(); }}
                  onMouseEnter={() => setSelectedIdx(idx)}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    padding: "6px 14px",
                    gap: 10,
                    cursor: "pointer",
                    background: isSelected ? "var(--bg3)" : "transparent",
                    borderLeft: isSelected ? "2px solid var(--accent)" : "2px solid transparent",
                  }}
                >
                  <span style={{
                    fontFamily: "monospace",
                    fontWeight: 600,
                    fontSize: 13,
                    color: "var(--text)",
                    minWidth: 0,
                    flex: "0 0 auto",
                    maxWidth: 220,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}>
                    {highlight(r.key, query)}
                  </span>
                  <span style={{
                    fontSize: 11,
                    color: "var(--accent)",
                    flexShrink: 0,
                  }}>
                    {highlight(r.actual_type, query)}
                  </span>
                  <span style={{
                    fontSize: 10,
                    color: "var(--text-muted)",
                    marginLeft: "auto",
                    flexShrink: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    maxWidth: 180,
                    direction: "rtl",
                    textAlign: "left",
                  }}>
                    {r.file_path}
                  </span>
                </div>
              );
            })
          )}
        </div>

        {/* Footer */}
        <div style={{
          padding: "6px 14px",
          borderTop: "1px solid var(--border)",
          display: "flex",
          gap: 12,
          fontSize: 11,
          color: "var(--text-muted)",
        }}>
          <span>↑↓ navigate</span>
          <span>↵ open</span>
          <span style={{ marginLeft: "auto" }}>{filtered.length} / {records.length} records</span>
        </div>
      </div>
    </div>
  );
}
