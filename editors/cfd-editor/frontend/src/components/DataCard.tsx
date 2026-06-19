import { useState, useRef, useEffect } from "react";
import type { FieldValue, DictKey } from "../bindings";

// ─── helpers ──────────────────────────────────────────────────────────────────

function middleTruncate(s: string, max: number): string {
  if (s.length <= max) return s;
  const half = Math.floor((max - 1) / 2);
  return s.slice(0, half) + "…" + s.slice(s.length - (max - half - 1));
}

function scalarTypeName(v: FieldValue): string {
  switch (v.kind) {
    case "Null": return "null";
    case "Bool": return "bool";
    case "Int": return "int";
    case "Float": return "float";
    case "Str": return "str";
    case "Enum": return v.enum_name;
    case "Ref": return "ref";
    default: return v.kind;
  }
}

function isScalar(v: FieldValue): boolean {
  return ["Null", "Bool", "Int", "Float", "Str", "Enum", "Ref"].includes(v.kind);
}

function dictKeyStr(k: DictKey): string {
  switch (k.kind) {
    case "Str": return k.v;
    case "Int": return String(k.v);
    case "Enum": return k.variant;
  }
}

function parseEditedValue(raw: string, originalKind: FieldValue["kind"]): FieldValue {
  const trimmed = raw.trim();
  if (trimmed === "null") return { kind: "Null" };
  if (trimmed === "true") return { kind: "Bool", v: true };
  if (trimmed === "false") return { kind: "Bool", v: false };
  // Try int
  if (/^-?\d+$/.test(trimmed)) {
    try {
      return { kind: "Int", v: BigInt(trimmed) };
    } catch {
      // fall through
    }
  }
  // Try float
  if (/^-?\d*\.?\d+([eE][+-]?\d+)?$/.test(trimmed) && trimmed.includes(".")) {
    const f = parseFloat(trimmed);
    if (!isNaN(f)) return { kind: "Float", v: f };
  }
  // Enum: if original was enum, keep as enum variant (just update variant name)
  // We can't reconstruct the full enum type here easily, so fall back to Str
  void originalKind;
  return { kind: "Str", v: raw };
}

// ─── Compact renderer ─────────────────────────────────────────────────────────

function renderCompact(v: FieldValue): React.ReactNode {
  switch (v.kind) {
    case "Null":
      return <span style={{ color: "var(--text-muted)" }}>—</span>;

    case "Bool":
      return <span style={{ color: "#bd93f9" }}>{String(v.v)}</span>;

    case "Int":
      return <span style={{ color: "#f1fa8c" }}>{String(v.v)}</span>;

    case "Float":
      return <span style={{ color: "#f1fa8c" }}>{String(v.v)}</span>;

    case "Str":
      return <span>{v.v}</span>;

    case "Enum":
      return <span style={{ color: "#50fa7b" }}>{v.variant}</span>;

    case "Ref": {
      const full = v.target_file
        ? `${v.target_file}:${v.target_type}.${v.target_key}`
        : `${v.target_key}`;
      const display = v.target_file
        ? `→ ${v.target_type}.${middleTruncate(v.target_key, 20)}`
        : `→ ${middleTruncate(v.target_key, 28)}`;
      return (
        <span
          title={full}
          style={{ color: "var(--accent)", fontStyle: "italic", cursor: "default" }}
        >
          {middleTruncate(display, 28)}
        </span>
      );
    }

    case "Object":
      return <span style={{ color: "var(--text-muted)" }}>{v.actual_type}</span>;

    case "Array": {
      const items = v.items;
      if (items.length === 0) return <span style={{ color: "var(--text-muted)" }}>[]</span>;
      const allScalar = items.every(isScalar);
      if (allScalar && items.length <= 6) {
        const parts = items.map(item => {
          switch (item.kind) {
            case "Null": return "null";
            case "Bool": return String(item.v);
            case "Int": return String(item.v);
            case "Float": return String(item.v);
            case "Str": return item.v;
            case "Enum": return item.variant;
            default: return "…";
          }
        });
        const inline = `[${parts.join(", ")}]`;
        if (inline.length <= 100) {
          return <span style={{ color: "var(--text-muted)" }}>{inline}</span>;
        }
      }
      const typeName = items.length > 0 ? scalarTypeName(items[0]) : "?";
      return (
        <span style={{ color: "var(--text-muted)" }}>
          [{typeName} × {items.length}]
        </span>
      );
    }

    case "Dict": {
      const n = v.entries.length;
      if (n === 0) return <span style={{ color: "var(--text-muted)" }}>{"{}"}</span>;
      const kType = n > 0 ? v.entries[0].key.kind : "?";
      const vType = n > 0 ? scalarTypeName(v.entries[0].value) : "?";
      return (
        <span style={{ color: "var(--text-muted)" }}>
          {`{${kType}: ${vType} × ${n}}`}
        </span>
      );
    }
  }
}

// ─── Inline editor ────────────────────────────────────────────────────────────

interface InlineEditorProps {
  value: FieldValue;
  onCommit: (v: FieldValue) => void;
  onCancel: () => void;
}

function InlineEditor({ value, onCommit, onCancel }: InlineEditorProps) {
  const initialStr = (): string => {
    switch (value.kind) {
      case "Null": return "null";
      case "Bool": return String(value.v);
      case "Int": return String(value.v);
      case "Float": return String(value.v);
      case "Str": return value.v;
      case "Enum": return value.variant;
      default: return "";
    }
  };

  const [text, setText] = useState(initialStr);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  return (
    <input
      ref={inputRef}
      value={text}
      onChange={e => setText(e.target.value)}
      onKeyDown={e => {
        if (e.key === "Enter") onCommit(parseEditedValue(text, value.kind));
        if (e.key === "Escape") onCancel();
        e.stopPropagation();
      }}
      onBlur={() => onCommit(parseEditedValue(text, value.kind))}
      onClick={e => e.stopPropagation()}
      style={{
        background: "var(--bg3)",
        border: "1px solid var(--accent)",
        borderRadius: 3,
        color: "var(--text)",
        fontSize: 12,
        fontFamily: "monospace",
        padding: "1px 4px",
        width: "100%",
        outline: "none",
      }}
    />
  );
}

// ─── Expanded renderer ────────────────────────────────────────────────────────

interface ExpandedProps {
  value: FieldValue;
  depth: number;
  onEdit?: (newValue: FieldValue) => void;
  label?: string;
}

function ExpandedValue({ value, depth, onEdit, label }: ExpandedProps) {
  const MAX_DEPTH = 5;
  const [editing, setEditing] = useState(false);
  const [collapsed, setCollapsed] = useState<boolean>(() => {
    if (depth > 0) {
      // Auto-expand small collections
      if (value.kind === "Array" && value.items.length <= 3) return false;
      if (value.kind === "Dict" && value.entries.length <= 3) return false;
      if (value.kind === "Object" && value.fields.length <= 3) return false;
      return false;
    }
    return true; // depth=0 starts collapsed
  });

  const marginLeft = depth > 0 ? depth * 10 : 0;

  if (depth >= MAX_DEPTH) {
    return (
      <div style={{ marginLeft, display: "flex", alignItems: "center", gap: 6, padding: "2px 0" }}>
        {label && <span style={{ color: "var(--text-muted)", minWidth: 80, fontSize: 12 }}>{label}:</span>}
        {renderCompact(value)}
      </div>
    );
  }

  // Scalar values
  if (isScalar(value)) {
    const isRef = value.kind === "Ref";
    const canEdit = !!onEdit && !isRef;

    if (canEdit && editing) {
      return (
        <div style={{ marginLeft, display: "flex", alignItems: "center", gap: 6, padding: "2px 0" }}>
          {label && <span style={{ color: "var(--text-muted)", minWidth: 80, fontSize: 12 }}>{label}:</span>}
          <InlineEditor
            value={value}
            onCommit={v => { onEdit(v); setEditing(false); }}
            onCancel={() => setEditing(false)}
          />
        </div>
      );
    }

    return (
      <div
        style={{ marginLeft, display: "flex", alignItems: "center", gap: 6, padding: "2px 0", cursor: canEdit ? "pointer" : "default" }}
        onClick={canEdit ? () => setEditing(true) : undefined}
        title={canEdit ? "Click to edit" : undefined}
      >
        {label && <span style={{ color: "var(--text-muted)", minWidth: 80, fontSize: 12 }}>{label}:</span>}
        {renderCompact(value)}
        {canEdit && <span style={{ color: "var(--text-muted)", fontSize: 10, opacity: 0.5 }}>✎</span>}
      </div>
    );
  }

  // Object
  if (value.kind === "Object") {
    return (
      <div style={{ marginLeft }}>
        <div
          style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0", cursor: "pointer", userSelect: "none" }}
          onClick={() => setCollapsed(c => !c)}
        >
          {label && <span style={{ color: "var(--text-muted)", minWidth: 80, fontSize: 12 }}>{label}:</span>}
          <span style={{ color: "var(--text-muted)", fontSize: 10 }}>{collapsed ? "▶" : "▼"}</span>
          <span style={{ color: "var(--text-muted)", fontStyle: "italic" }}>{value.actual_type}</span>
        </div>
        {!collapsed && value.fields.map(field => (
          <ExpandedValue
            key={field.name}
            value={field.value}
            depth={depth + 1}
            label={field.name}
            onEdit={onEdit ? (nv) => onEdit({
              kind: "Object",
              actual_type: value.actual_type,
              fields: value.fields.map(f => f.name === field.name ? { ...f, value: nv } : f),
            }) : undefined}
          />
        ))}
      </div>
    );
  }

  // Array
  if (value.kind === "Array") {
    return (
      <div style={{ marginLeft }}>
        <div
          style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0", cursor: "pointer", userSelect: "none" }}
          onClick={() => setCollapsed(c => !c)}
        >
          {label && <span style={{ color: "var(--text-muted)", minWidth: 80, fontSize: 12 }}>{label}:</span>}
          <span style={{ color: "var(--text-muted)", fontSize: 10 }}>{collapsed ? "▶" : "▼"}</span>
          <span style={{ color: "var(--text-muted)" }}>[{value.items.length} items]</span>
        </div>
        {!collapsed && value.items.map((item, idx) => (
          <ExpandedValue
            key={idx}
            value={item}
            depth={depth + 1}
            label={String(idx)}
            onEdit={onEdit ? (nv) => {
              const newItems = [...value.items];
              newItems[idx] = nv;
              onEdit({ kind: "Array", items: newItems });
            } : undefined}
          />
        ))}
      </div>
    );
  }

  // Dict
  if (value.kind === "Dict") {
    return (
      <div style={{ marginLeft }}>
        <div
          style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0", cursor: "pointer", userSelect: "none" }}
          onClick={() => setCollapsed(c => !c)}
        >
          {label && <span style={{ color: "var(--text-muted)", minWidth: 80, fontSize: 12 }}>{label}:</span>}
          <span style={{ color: "var(--text-muted)", fontSize: 10 }}>{collapsed ? "▶" : "▼"}</span>
          <span style={{ color: "var(--text-muted)" }}>{"{"}  {value.entries.length} entries {"}"}</span>
        </div>
        {!collapsed && value.entries.map((entry, idx) => (
          <ExpandedValue
            key={idx}
            value={entry.value}
            depth={depth + 1}
            label={dictKeyStr(entry.key)}
            onEdit={onEdit ? (nv) => {
              const newEntries = [...value.entries];
              newEntries[idx] = { ...entry, value: nv };
              onEdit({ kind: "Dict", entries: newEntries });
            } : undefined}
          />
        ))}
      </div>
    );
  }

  return renderCompact(value);
}

// ─── Node mode renderer ───────────────────────────────────────────────────────

interface NodeModeProps {
  value: FieldValue;
}

function NodeModeValue({ value }: NodeModeProps) {
  if (value.kind !== "Object") {
    return <div style={{ fontSize: 11 }}>{renderCompact(value)}</div>;
  }

  const fields = value.fields;
  const shown = fields.slice(0, 4);
  const remaining = fields.length - shown.length;

  return (
    <div>
      {shown.map(f => (
        <div key={f.name} style={{ display: "flex", gap: 4, fontSize: 11, lineHeight: "18px" }}>
          <span style={{ color: "var(--text-muted)", flexShrink: 0 }}>{f.name}:</span>
          {renderCompact(f.value)}
        </div>
      ))}
      {remaining > 0 && (
        <div style={{ color: "var(--text-muted)", fontSize: 11 }}>…{remaining} more</div>
      )}
    </div>
  );
}

// ─── Public component ─────────────────────────────────────────────────────────

export interface DataCardProps {
  value: FieldValue;
  mode: "compact" | "expanded" | "node";
  depth?: number;
  onEdit?: (newValue: FieldValue) => void;
  label?: string;
}

export function DataCard({ value, mode, depth = 0, onEdit, label }: DataCardProps) {
  if (mode === "compact") {
    return <span style={{ fontFamily: "monospace", fontSize: 12 }}>{renderCompact(value)}</span>;
  }

  if (mode === "node") {
    return <NodeModeValue value={value} />;
  }

  // expanded
  return (
    <div style={{ fontFamily: "monospace", fontSize: 12 }}>
      <ExpandedValue value={value} depth={depth} onEdit={onEdit} label={label} />
    </div>
  );
}
