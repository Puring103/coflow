import { useState, useCallback } from "react";
import type { FileRecords, FieldPathSegment, FieldValue, FieldCell } from "../bindings";
import type { Route } from "../router";
import { DataCard } from "./DataCard";
import { ContextMenu, type ContextMenuState } from "./ContextMenu";

interface RecordViewProps {
  sessionId: number;
  filePath: string;
  recordKey: string;
  fileRecords: FileRecords | null;
  onWriteField: (
    sessionId: number,
    filePath: string,
    recordKey: string,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue
  ) => Promise<void>;
  onNavigate: (route: Route) => void;
}

export function RecordView({
  sessionId,
  filePath,
  recordKey,
  fileRecords,
  onWriteField,
  onNavigate,
}: RecordViewProps) {
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);

  const record = fileRecords?.records.find(r => r.key === recordKey) ?? null;
  const allRecords = fileRecords?.records ?? [];

  const handleFieldEdit = useCallback(async (field: FieldCell, newValue: FieldValue) => {
    await onWriteField(sessionId, filePath, recordKey, [{ kind: "Field", name: field.name }], newValue);
  }, [sessionId, filePath, recordKey, onWriteField]);

  const handleFieldContextMenu = useCallback((e: React.MouseEvent, field: FieldCell) => {
    if (field.value.kind !== "Ref") return;
    e.preventDefault();
    const refValue = field.value;
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        {
          label: "跳转到引用记录",
          onClick: () => {
            const targetFile = refValue.target_file ?? filePath;
            onNavigate({ view: "record", file: targetFile, recordKey: refValue.target_key });
          },
        },
      ],
    });
  }, [filePath, onNavigate]);

  return (
    <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
      {/* Left sidebar: record list */}
      <div style={{
        width: 200,
        borderRight: "1px solid var(--border)",
        overflowY: "auto",
        flexShrink: 0,
        background: "var(--bg2)",
      }}>
        <div style={{
          padding: "6px 8px",
          fontSize: 11,
          fontWeight: 600,
          color: "var(--text-muted)",
          textTransform: "uppercase",
          letterSpacing: 1,
          borderBottom: "1px solid var(--border)",
        }}>
          Records
        </div>
        {allRecords.map(r => (
          <div
            key={r.key}
            onClick={() => onNavigate({ view: "record", file: filePath, recordKey: r.key })}
            style={{
              padding: "5px 10px",
              cursor: "pointer",
              background: r.key === recordKey ? "var(--bg3)" : "transparent",
              borderLeft: r.key === recordKey ? "2px solid var(--accent)" : "2px solid transparent",
              fontSize: 12,
              fontFamily: "monospace",
              color: r.key === recordKey ? "var(--text)" : "var(--text-muted)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
            onMouseEnter={e => { if (r.key !== recordKey) e.currentTarget.style.background = "var(--bg3)"; }}
            onMouseLeave={e => { if (r.key !== recordKey) e.currentTarget.style.background = "transparent"; }}
            title={r.key}
          >
            {r.key}
          </div>
        ))}
        {allRecords.length === 0 && (
          <div style={{ padding: "8px 12px", color: "var(--text-muted)", fontSize: 12 }}>No records</div>
        )}
      </div>

      {/* Main content */}
      <div style={{ flex: 1, overflow: "auto", padding: 16 }}>
        {record ? (
          <>
            {/* Header */}
            <div style={{ marginBottom: 16, paddingBottom: 12, borderBottom: "1px solid var(--border)" }}>
              <div style={{
                fontFamily: "monospace",
                fontSize: 18,
                fontWeight: 700,
                color: "var(--text)",
              }}>
                {record.key}
              </div>
              <div style={{ color: "var(--text-muted)", fontSize: 12, marginTop: 4 }}>
                {record.actual_type}
              </div>
            </div>

            {/* Fields */}
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {record.fields.map(field => (
                <div
                  key={field.name}
                  onContextMenu={e => handleFieldContextMenu(e, field)}
                  style={{
                    display: "flex",
                    alignItems: "flex-start",
                    gap: 8,
                    padding: "4px 8px",
                    borderRadius: 4,
                  }}
                  onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
                  onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
                >
                  <span style={{
                    minWidth: 140,
                    color: "var(--text-muted)",
                    fontSize: 12,
                    fontFamily: "monospace",
                    paddingTop: 3,
                    flexShrink: 0,
                  }}>
                    {field.name}
                  </span>
                  <div style={{ flex: 1 }}>
                    <DataCard
                      mode="expanded"
                      value={field.value}
                      depth={0}
                      label={undefined}
                      onEdit={field.value.kind !== "Ref" ? (nv) => handleFieldEdit(field, nv) : undefined}
                    />
                  </div>
                </div>
              ))}
              {record.fields.length === 0 && (
                <div style={{ color: "var(--text-muted)", fontSize: 12, padding: 8 }}>No fields</div>
              )}
            </div>
          </>
        ) : (
          <div style={{ color: "var(--text-muted)", padding: 16, fontSize: 13 }}>
            Record <code style={{ fontFamily: "monospace" }}>{recordKey}</code> not found.
          </div>
        )}
      </div>

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
