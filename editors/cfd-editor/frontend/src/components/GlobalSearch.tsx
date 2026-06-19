import { useState, useEffect, useRef, useCallback } from "react";
import type { SearchHit } from "../bindings";
import type { Route } from "../router";
import { api } from "../api";

interface GlobalSearchProps {
  sessionId: number;
  onNavigate: (route: Route) => void;
  onClose: () => void;
}

export function GlobalSearch({ sessionId, onNavigate, onClose }: GlobalSearchProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedIdx, setSelectedIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const runSearch = useCallback((q: string) => {
    if (!q.trim()) { setResults([]); setLoading(false); return; }
    setLoading(true);
    api.searchRecords(sessionId, q.trim(), 80)
      .then(hits => { setResults(hits); setSelectedIdx(0); setLoading(false); })
      .catch(() => { setResults([]); setLoading(false); });
  }, [sessionId]);

  const handleQueryChange = (q: string) => {
    setQuery(q);
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    if (!q.trim()) { setResults([]); setLoading(false); return; }
    setLoading(true);
    searchTimerRef.current = setTimeout(() => { runSearch(q); }, 200);
  };

  const handleSelect = useCallback((hit: SearchHit) => {
    const topField = hit.match_field !== "key" ? hit.match_field.split(".")[0] : undefined;
    onNavigate({ view: "record", file: hit.file_path, recordKey: hit.key, ...(topField ? { fieldSearch: topField } : {}) });
    onClose();
  }, [onNavigate, onClose]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); onClose(); return; }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIdx(i => Math.min(i + 1, results.length - 1));
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIdx(i => Math.max(i - 1, 0));
      }
      if (e.key === "Enter" && results[selectedIdx]) {
        e.preventDefault();
        handleSelect(results[selectedIdx]);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [results, selectedIdx, onClose, handleSelect]);

  // Scroll selected item into view
  useEffect(() => {
    const el = listRef.current?.children[selectedIdx] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest" });
  }, [selectedIdx]);

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.55)",
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        zIndex: 5000,
        paddingTop: 80,
      }}
      onMouseDown={e => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div
        style={{
          background: "var(--bg2)",
          border: "1px solid var(--border)",
          borderRadius: 10,
          width: 600,
          maxWidth: "90vw",
          display: "flex",
          flexDirection: "column",
          boxShadow: "0 8px 40px rgba(0,0,0,0.5)",
          overflow: "hidden",
        }}
        onMouseDown={e => e.stopPropagation()}
      >
        {/* Search input */}
        <div style={{ display: "flex", alignItems: "center", padding: "10px 14px", borderBottom: "1px solid var(--border)", gap: 8 }}>
          <span style={{ color: "var(--text-muted)", fontSize: 16 }}>⌕</span>
          <input
            ref={inputRef}
            value={query}
            onChange={e => handleQueryChange(e.target.value)}
            placeholder="Search records by key or field value…"
            style={{
              flex: 1,
              background: "transparent",
              border: "none",
              outline: "none",
              color: "var(--text)",
              fontSize: 15,
              fontFamily: "monospace",
            }}
          />
          {loading && (
            <span style={{ color: "var(--text-muted)", fontSize: 12 }}>…</span>
          )}
          {!loading && query && (
            <span style={{ color: "var(--text-muted)", fontSize: 11 }}>
              {results.length} {results.length === 80 ? "(truncated)" : "result" + (results.length !== 1 ? "s" : "")}
            </span>
          )}
        </div>

        {/* Results list */}
        <div
          ref={listRef}
          style={{
            maxHeight: 400,
            overflowY: "auto",
          }}
        >
          {results.length === 0 && query && !loading && (
            <div style={{ padding: "16px 16px", color: "var(--text-muted)", fontSize: 13 }}>
              No results for <strong style={{ color: "var(--text)" }}>{query}</strong>
            </div>
          )}
          {results.length === 0 && !query && (
            <div style={{ padding: "16px 16px", color: "var(--text-muted)", fontSize: 12 }}>
              Search across all record keys and field values in the project.
              <br />
              <span style={{ opacity: 0.7 }}>↑↓ to navigate, Enter to open, Esc to close</span>
            </div>
          )}
          {results.map((hit, idx) => {
            const isSelected = idx === selectedIdx;
            const isKeyMatch = hit.match_field === "key";
            return (
              <div
                key={`${hit.key}:${idx}`}
                onClick={() => handleSelect(hit)}
                onMouseEnter={() => setSelectedIdx(idx)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  padding: "7px 14px",
                  cursor: "pointer",
                  background: isSelected ? "var(--bg3)" : "transparent",
                  borderLeft: isSelected ? "2px solid var(--accent)" : "2px solid transparent",
                }}
              >
                {/* Type badge */}
                <span style={{
                  fontSize: 10,
                  padding: "1px 5px",
                  background: "var(--bg3)",
                  border: "1px solid var(--border)",
                  borderRadius: 4,
                  color: "var(--text-muted)",
                  flexShrink: 0,
                  fontFamily: "monospace",
                }}>
                  {hit.actual_type}
                </span>
                {/* Key */}
                <span style={{ fontFamily: "monospace", fontWeight: 600, fontSize: 13, color: "var(--text)", flexShrink: 0 }}>
                  {hit.key}
                </span>
                {/* Match context */}
                {!isKeyMatch && (
                  <span style={{ color: "var(--text-muted)", fontSize: 12, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    <span style={{ color: "var(--accent)", marginRight: 4 }}>.{hit.match_field}</span>
                    = <span style={{ fontFamily: "monospace" }}>{hit.match_value.length > 60 ? hit.match_value.slice(0, 60) + "…" : hit.match_value}</span>
                  </span>
                )}
                {/* File path */}
                <span style={{ marginLeft: "auto", color: "var(--text-muted)", fontSize: 11, flexShrink: 0, fontFamily: "monospace" }}>
                  {hit.file_path.split("/").pop() ?? hit.file_path}
                </span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
