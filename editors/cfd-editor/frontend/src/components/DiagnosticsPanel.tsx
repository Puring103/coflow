import { useState, useEffect, useRef } from "react";
import type { DiagnosticItem } from "../bindings";
import type { Route } from "../router";

interface DiagnosticsPanelProps {
  diagnostics: DiagnosticItem[];
  onNavigate?: (route: Route) => void;
  currentFile?: string | null;
}

type SeverityFilter = "all" | "error" | "warning" | "info";

function severityColor(severity: string): string {
  switch (severity.toLowerCase()) {
    case "error": return "var(--error)";
    case "warning": return "var(--warning)";
    default: return "var(--accent)";
  }
}

function severityIcon(severity: string): string {
  switch (severity.toLowerCase()) {
    case "error": return "✕";
    case "warning": return "⚠";
    default: return "ℹ";
  }
}

function severityRank(severity: string): number {
  switch (severity.toLowerCase()) {
    case "error": return 0;
    case "warning": return 1;
    default: return 2;
  }
}

export function DiagnosticsPanel({ diagnostics, onNavigate, currentFile }: DiagnosticsPanelProps) {
  const [expanded, setExpanded] = useState(false);
  const [filter, setFilter] = useState<SeverityFilter>("all");
  const [fileOnly, setFileOnly] = useState(false);
  const prevErrorCountRef = useRef(0);
  const listRef = useRef<HTMLDivElement>(null);

  const errors = diagnostics.filter(d => d.severity.toLowerCase() === "error").length;
  const warnings = diagnostics.filter(d => d.severity.toLowerCase() === "warning").length;
  const infos = diagnostics.filter(d => d.severity.toLowerCase() !== "error" && d.severity.toLowerCase() !== "warning").length;

  // Auto-expand when new errors appear, then scroll list to top
  useEffect(() => {
    if (errors > prevErrorCountRef.current && errors > 0) {
      setExpanded(true);
      // Scroll to top after the panel renders
      requestAnimationFrame(() => {
        listRef.current?.scrollTo({ top: 0 });
      });
    }
    prevErrorCountRef.current = errors;
  }, [errors]);

  // When current file changes and fileOnly is on, reset scroll
  useEffect(() => {
    if (fileOnly && expanded) {
      requestAnimationFrame(() => { listRef.current?.scrollTo({ top: 0 }); });
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentFile]);

  const baseFiltered = fileOnly && currentFile
    ? diagnostics.filter(d => d.file_path === currentFile)
    : diagnostics;

  const filtered = baseFiltered
    .filter(d => {
      if (filter === "all") return true;
      if (filter === "info") return d.severity.toLowerCase() !== "error" && d.severity.toLowerCase() !== "warning";
      return d.severity.toLowerCase() === filter;
    })
    .sort((a, b) => severityRank(a.severity) - severityRank(b.severity));

  const handleItemClick = (item: DiagnosticItem) => {
    if (!onNavigate || !item.file_path) return;
    if (item.record_key) {
      onNavigate({ view: "record", file: item.file_path, recordKey: item.record_key });
    } else {
      onNavigate({ view: "table", file: item.file_path });
    }
  };

  const isNavigable = (item: DiagnosticItem) => !!onNavigate && !!item.file_path;

  const FILTER_BTNS: { key: SeverityFilter; label: string; count: number; active: boolean }[] = [
    { key: "all", label: "All", count: diagnostics.length, active: filter === "all" },
    { key: "error", label: "Errors", count: errors, active: filter === "error" },
    { key: "warning", label: "Warnings", count: warnings, active: filter === "warning" },
    { key: "info", label: "Info", count: infos, active: filter === "info" },
  ];

  return (
    <div style={{
      borderTop: "1px solid var(--border)",
      background: "var(--bg2)",
      flexShrink: 0,
    }}>
      {/* Header bar */}
      <div style={{ display: "flex", alignItems: "center" }}>
        <div
          onClick={() => setExpanded(e => !e)}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "4px 12px",
            cursor: "pointer",
            userSelect: "none",
            height: 28,
            flex: 1,
          }}
        >
          <span style={{ color: "var(--text-muted)", fontSize: 11 }}>
            {expanded ? "▼" : "▶"}
          </span>
          <span style={{ fontWeight: 500, fontSize: 12, color: "var(--text-muted)" }}>PROBLEMS</span>
          {errors > 0 && (
            <span style={{
              background: "var(--error)",
              color: "#fff",
              borderRadius: 10,
              padding: "0 6px",
              fontSize: 11,
              fontWeight: 600,
              lineHeight: "18px",
            }}>{errors}</span>
          )}
          {warnings > 0 && (
            <span style={{
              background: "var(--warning)",
              color: "#000",
              borderRadius: 10,
              padding: "0 6px",
              fontSize: 11,
              fontWeight: 600,
              lineHeight: "18px",
            }}>{warnings}</span>
          )}
          {infos > 0 && (
            <span style={{
              background: "var(--accent)",
              color: "#fff",
              borderRadius: 10,
              padding: "0 6px",
              fontSize: 11,
              fontWeight: 600,
              lineHeight: "18px",
            }}>{infos}</span>
          )}
          {diagnostics.length === 0 && (
            <span style={{ color: "var(--text-muted)", fontSize: 12 }}>No problems</span>
          )}
        </div>

        {/* Severity filter tabs + file filter — only show when expanded */}
        {expanded && (
          <div style={{ display: "flex", gap: 2, padding: "0 8px", alignItems: "center" }}>
            {currentFile && (
              <button
                onClick={e => { e.stopPropagation(); setFileOnly(f => !f); }}
                title={fileOnly ? "Show all files" : `Show only: ${currentFile}`}
                style={{
                  fontSize: 11,
                  padding: "2px 8px",
                  background: fileOnly ? "var(--accent)" : "transparent",
                  border: fileOnly ? "1px solid var(--accent)" : "1px solid var(--border)",
                  borderRadius: 4,
                  color: fileOnly ? "#fff" : "var(--text-muted)",
                  cursor: "pointer",
                  marginRight: 4,
                }}
              >
                This file
              </button>
            )}
            {FILTER_BTNS.filter(b => b.count > 0 || b.key === "all").map(btn => (
              <button
                key={btn.key}
                onClick={e => { e.stopPropagation(); setFilter(btn.key); }}
                style={{
                  fontSize: 11,
                  padding: "2px 8px",
                  background: btn.active ? "var(--bg3)" : "transparent",
                  border: btn.active ? "1px solid var(--border)" : "1px solid transparent",
                  borderRadius: 4,
                  color: btn.active ? "var(--text)" : "var(--text-muted)",
                  cursor: "pointer",
                }}
              >
                {btn.label}
                {btn.count > 0 && (
                  <span style={{ marginLeft: 4, color: "var(--text-muted)" }}>({btn.count})</span>
                )}
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Expanded list */}
      {expanded && (
        <div ref={listRef} style={{
          maxHeight: 220,
          overflowY: "auto",
          borderTop: "1px solid var(--border)",
        }}>
          {filtered.length === 0 ? (
            <div style={{ padding: "8px 16px", color: "var(--text-muted)", fontSize: 12 }}>
              {diagnostics.length === 0
                ? "No problems detected."
                : fileOnly && currentFile
                  ? `No problems in ${currentFile.split("/").pop() ?? currentFile}.`
                  : "No items match the current filter."}
            </div>
          ) : (
            filtered.map((item, idx) => {
              const navigable = isNavigable(item);
              return (
                <div
                  key={`${item.code}:${item.record_key ?? ""}:${item.message}:${idx}`}
                  onClick={() => handleItemClick(item)}
                  style={{
                    display: "flex",
                    alignItems: "flex-start",
                    gap: 8,
                    padding: "4px 12px",
                    borderBottom: "1px solid var(--bg3)",
                    fontSize: 12,
                    cursor: navigable ? "pointer" : "default",
                  }}
                  onMouseEnter={e => { if (navigable) e.currentTarget.style.background = "var(--bg3)"; }}
                  onMouseLeave={e => { if (navigable) e.currentTarget.style.background = "transparent"; }}
                  title={navigable ? `Click to navigate to ${item.record_key ?? item.file_path}` : undefined}
                >
                  <span style={{ color: severityColor(item.severity), flexShrink: 0, marginTop: 1 }}>
                    {severityIcon(item.severity)}
                  </span>
                  <span style={{
                    color: severityColor(item.severity),
                    fontWeight: 600,
                    flexShrink: 0,
                    fontFamily: "monospace",
                    fontSize: 11,
                  }}>
                    {item.code}
                  </span>
                  <span style={{
                    color: "var(--text-muted)",
                    flexShrink: 0,
                    fontFamily: "monospace",
                    fontSize: 10,
                    opacity: 0.7,
                    alignSelf: "center",
                  }}>
                    [{item.stage}]
                  </span>
                  <span style={{ color: "var(--text)", flex: 1 }}>{item.message}</span>
                  {(item.file_path || item.record_key) && (
                    <span style={{
                      color: navigable ? "var(--accent)" : "var(--text-muted)",
                      flexShrink: 0,
                      fontFamily: "monospace",
                      fontSize: 11,
                      textDecoration: navigable ? "underline" : "none",
                    }}>
                      {item.file_path && <span>{item.file_path}</span>}
                      {item.record_key && <span> [{item.record_key}]</span>}
                      {item.field_path && <span style={{ color: "var(--text-muted)" }}> .{item.field_path}</span>}
                    </span>
                  )}
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
