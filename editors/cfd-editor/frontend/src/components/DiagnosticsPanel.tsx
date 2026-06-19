import { useState } from "react";
import type { DiagnosticItem } from "../bindings";

interface DiagnosticsPanelProps {
  diagnostics: DiagnosticItem[];
}

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

export function DiagnosticsPanel({ diagnostics }: DiagnosticsPanelProps) {
  const [expanded, setExpanded] = useState(false);

  const errors = diagnostics.filter(d => d.severity.toLowerCase() === "error").length;
  const warnings = diagnostics.filter(d => d.severity.toLowerCase() === "warning").length;
  const infos = diagnostics.filter(d => d.severity.toLowerCase() !== "error" && d.severity.toLowerCase() !== "warning").length;

  return (
    <div style={{
      borderTop: "1px solid var(--border)",
      background: "var(--bg2)",
      flexShrink: 0,
    }}>
      {/* Header bar */}
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

      {/* Expanded list */}
      {expanded && (
        <div style={{
          maxHeight: 200,
          overflowY: "auto",
          borderTop: "1px solid var(--border)",
        }}>
          {diagnostics.length === 0 ? (
            <div style={{ padding: "8px 16px", color: "var(--text-muted)", fontSize: 12 }}>
              No problems detected.
            </div>
          ) : (
            diagnostics.map((item, idx) => (
              <div key={idx} style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 8,
                padding: "4px 12px",
                borderBottom: "1px solid var(--bg3)",
                fontSize: 12,
              }}>
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
                <span style={{ color: "var(--text)", flex: 1 }}>{item.message}</span>
                {(item.file_path || item.record_key) && (
                  <span style={{ color: "var(--text-muted)", flexShrink: 0, fontFamily: "monospace", fontSize: 11 }}>
                    {item.file_path ?? ""}
                    {item.record_key ? ` [${item.record_key}]` : ""}
                    {item.field_path ? ` .${item.field_path}` : ""}
                  </span>
                )}
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}
