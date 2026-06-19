import { useState, useRef, useEffect } from "react";
import type { FieldValue, DictKey } from "../bindings";
import { api } from "../api";

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

function parseEditedValue(raw: string, original: FieldValue): FieldValue {
  const trimmed = raw.trim();
  // For enum fields, only update the variant name; preserve enum_name and int_value
  if (original.kind === "Enum") {
    return { kind: "Enum", enum_name: original.enum_name, variant: trimmed, int_value: original.int_value };
  }
  if (trimmed === "null") return { kind: "Null" };
  if (trimmed === "true") return { kind: "Bool", v: true };
  if (trimmed === "false") return { kind: "Bool", v: false };
  if (/^-?\d+$/.test(trimmed)) {
    const n = Number(trimmed);
    if (!isNaN(n)) return { kind: "Int", v: n };
  }
  if (/^-?\d*\.?\d+([eE][+-]?\d+)?$/.test(trimmed) && trimmed.includes(".")) {
    const f = parseFloat(trimmed);
    if (!isNaN(f)) return { kind: "Float", v: f };
  }
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

const INPUT_STYLE: React.CSSProperties = {
  background: "var(--bg3)",
  border: "1px solid var(--accent)",
  borderRadius: 3,
  color: "var(--text)",
  fontSize: 12,
  fontFamily: "monospace",
  padding: "1px 4px",
  outline: "none",
};

interface InlineEditorProps {
  value: FieldValue;
  onCommit: (v: FieldValue) => void;
  onCancel: () => void;
}

function valueToString(value: FieldValue): string {
  switch (value.kind) {
    case "Null": return "null";
    case "Bool": return String(value.v);
    case "Int": return String(value.v);
    case "Float": return String(value.v);
    case "Str": return value.v;
    case "Enum": return value.variant;
    default: return "";
  }
}

function InlineEditor({ value, onCommit, onCancel }: InlineEditorProps) {
  const initial = valueToString(value);
  const [text, setText] = useState(initial);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const commitIfChanged = () => {
    if (text !== initial) {
      onCommit(parseEditedValue(text, value));
    } else {
      onCancel();
    }
  };

  return (
    <input
      ref={inputRef}
      value={text}
      onChange={e => setText(e.target.value)}
      onKeyDown={e => {
        if (e.key === "Enter") { onCommit(parseEditedValue(text, value)); }
        if (e.key === "Escape") onCancel();
        e.stopPropagation();
      }}
      onBlur={commitIfChanged}
      onClick={e => e.stopPropagation()}
      style={{ ...INPUT_STYLE, width: "100%" }}
    />
  );
}

// ─── Ref editor ───────────────────────────────────────────────────────────────

interface RefEditorProps {
  value: FieldValue & { kind: "Ref" };
  sessionId?: number;
  onCommit: (v: FieldValue) => void;
  onCancel: () => void;
}

function RefEditor({ value, sessionId, onCommit, onCancel }: RefEditorProps) {
  const [key, setKey] = useState(value.target_key);
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  const listId = useRef(`ref-list-${Math.random().toString(36).slice(2)}`).current;

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  useEffect(() => {
    if (sessionId === undefined || !value.target_type) return;
    api.getRefTargets(sessionId, value.target_type).then(keys => {
      if (keys.length > 0) setSuggestions(keys);
    }).catch(() => {});
  }, [sessionId, value.target_type]);

  const commit = () =>
    onCommit({ kind: "Ref", target_type: value.target_type, target_key: key.trim(), target_file: value.target_file });

  const commitIfChanged = () => {
    if (key.trim() !== value.target_key) commit(); else onCancel();
  };

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
      <span style={{ color: "var(--accent)", fontSize: 12 }}>→</span>
      {suggestions.length > 0 && (
        <datalist id={listId}>
          {suggestions.map(s => <option key={s} value={s} />)}
        </datalist>
      )}
      <input
        ref={inputRef}
        value={key}
        list={suggestions.length > 0 ? listId : undefined}
        onChange={e => setKey(e.target.value)}
        onKeyDown={e => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") onCancel();
          e.stopPropagation();
        }}
        onBlur={commitIfChanged}
        onClick={e => e.stopPropagation()}
        style={{ ...INPUT_STYLE, flex: 1 }}
        placeholder="record_key"
      />
    </div>
  );
}

// ─── Enum editor ─────────────────────────────────────────────────────────────

interface EnumEditorProps {
  value: FieldValue & { kind: "Enum" };
  sessionId: number;
  onCommit: (v: FieldValue) => void;
  onCancel: () => void;
}

function EnumEditor({ value, sessionId, onCommit, onCancel }: EnumEditorProps) {
  const [variants, setVariants] = useState<string[]>([value.variant]);

  useEffect(() => {
    api.getEnumVariants(sessionId, value.enum_name).then(vs => {
      if (vs.length > 0) setVariants(vs);
    }).catch(() => {});
  }, [sessionId, value.enum_name]);

  return (
    <select
      value={value.variant}
      onChange={e => onCommit({ kind: "Enum", enum_name: value.enum_name, variant: e.target.value, int_value: value.int_value })}
      onKeyDown={e => { if (e.key === "Escape") onCancel(); e.stopPropagation(); }}
      // eslint-disable-next-line jsx-a11y/no-autofocus
      autoFocus
      style={{ ...INPUT_STYLE, width: "100%" }}
    >
      {variants.map(v => <option key={v} value={v}>{v}</option>)}
    </select>
  );
}

// ─── Dict entry row (with editable Str key) ──────────────────────────────────

interface DictEntryProps {
  entry: import("../bindings").DictEntry;
  depth: number;
  sessionId?: number;
  onEditValue?: (nv: FieldValue) => void;
  onEditKey?: (newKey: string) => void;
  onRemove?: () => void;
  onRefClick?: (targetFile: string | null, targetKey: string) => void;
}

function DictEntry({ entry, depth, sessionId, onEditValue, onEditKey, onRemove, onRefClick }: DictEntryProps) {
  const [editingKey, setEditingKey] = useState(false);
  const [keyText, setKeyText] = useState(entry.key.kind === "Str" ? entry.key.v : dictKeyStr(entry.key));
  const keyInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editingKey) keyInputRef.current?.select();
  }, [editingKey]);

  // Sync keyText if entry changes from outside
  useEffect(() => {
    if (!editingKey) setKeyText(entry.key.kind === "Str" ? entry.key.v : dictKeyStr(entry.key));
  }, [entry.key, editingKey]);

  const commitKey = () => {
    const trimmed = keyText.trim();
    if (trimmed && trimmed !== (entry.key.kind === "Str" ? entry.key.v : "")) {
      onEditKey?.(trimmed);
    }
    setEditingKey(false);
  };

  const keyLabel = editingKey && onEditKey ? (
    <input
      ref={keyInputRef}
      value={keyText}
      onChange={e => setKeyText(e.target.value)}
      onBlur={commitKey}
      onKeyDown={e => {
        if (e.key === "Enter") { commitKey(); e.stopPropagation(); }
        if (e.key === "Escape") { setEditingKey(false); e.stopPropagation(); }
        e.stopPropagation();
      }}
      onClick={e => e.stopPropagation()}
      style={{
        background: "var(--bg3)",
        border: "1px solid var(--accent)",
        borderRadius: 3,
        color: "var(--text)",
        fontSize: 11,
        fontFamily: "monospace",
        padding: "1px 4px",
        outline: "none",
        width: 80,
      }}
    />
  ) : (
    <span
      onClick={onEditKey ? (e) => { e.stopPropagation(); setEditingKey(true); } : undefined}
      title={onEditKey ? "Click to edit key" : undefined}
      style={{
        color: "var(--text-muted)",
        cursor: onEditKey ? "pointer" : "default",
        fontFamily: "monospace",
        fontSize: 11,
        borderBottom: onEditKey ? "1px dashed var(--text-muted)" : "none",
      }}
    >
      {dictKeyStr(entry.key)}
    </span>
  );

  const marginLeft = depth * 10;

  // For scalar values, render key: value inline on one row
  const isScalarVal = ["Null", "Bool", "Int", "Float", "Str", "Enum", "Ref"].includes(entry.value.kind);

  if (isScalarVal) {
    return (
      <div style={{ display: "flex", alignItems: "center" }}>
        <div style={{ flex: 1, marginLeft, display: "flex", alignItems: "center", gap: 6, padding: "2px 0" }}>
          {keyLabel}
          <span style={{ color: "var(--text-muted)", fontSize: 11 }}>:</span>
          <div style={{ flex: 1 }}>
            <ExpandedValue value={entry.value} depth={0} sessionId={sessionId} onEdit={onEditValue} onRefClick={onRefClick} />
          </div>
        </div>
        {onRemove && (
          <span
            onClick={onRemove}
            style={{ color: "var(--text-muted)", fontSize: 11, cursor: "pointer", padding: "4px 4px", flexShrink: 0 }}
            title="Remove entry"
          >×</span>
        )}
      </div>
    );
  }

  // For complex values, key label on its own line, value indented below
  return (
    <div>
      <div style={{ marginLeft, display: "flex", alignItems: "center", gap: 6, padding: "2px 0" }}>
        {keyLabel}
        <span style={{ color: "var(--text-muted)", fontSize: 11 }}>:</span>
        {onRemove && (
          <span
            onClick={onRemove}
            style={{ color: "var(--text-muted)", fontSize: 11, cursor: "pointer", marginLeft: "auto" }}
            title="Remove entry"
          >×</span>
        )}
      </div>
      <ExpandedValue value={entry.value} depth={depth + 1} sessionId={sessionId} onEdit={onEditValue} onRefClick={onRefClick} />
    </div>
  );
}

// ─── Expanded renderer ────────────────────────────────────────────────────────

interface ExpandedProps {
  value: FieldValue;
  depth: number;
  sessionId?: number;
  onEdit?: (newValue: FieldValue) => void;
  onRefClick?: (targetFile: string | null, targetKey: string) => void;
  label?: string;
}

function ExpandedValue({ value, depth, sessionId, onEdit, onRefClick, label }: ExpandedProps) {
  const MAX_DEPTH = 5;
  const [editing, setEditing] = useState(false);
  const [collapsed, setCollapsed] = useState<boolean>(() => {
    // Auto-expand small collections at any depth
    if (value.kind === "Array") return value.items.length > 6;
    if (value.kind === "Dict") return value.entries.length > 6;
    if (value.kind === "Object") return value.fields.length > 8;
    return false;
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

  // Scalar values (including Ref)
  if (isScalar(value)) {
    const isRef = value.kind === "Ref";
    const canEdit = !!onEdit;

    if (canEdit && editing) {
      return (
        <div style={{ marginLeft, display: "flex", alignItems: "center", gap: 6, padding: "2px 0" }}>
          {label && <span style={{ color: "var(--text-muted)", minWidth: 80, fontSize: 12 }}>{label}:</span>}
          <div style={{ flex: 1 }}>
            {isRef ? (
              <RefEditor
                value={value as FieldValue & { kind: "Ref" }}
                sessionId={sessionId}
                onCommit={v => { onEdit(v); setEditing(false); }}
                onCancel={() => setEditing(false)}
              />
            ) : value.kind === "Enum" && sessionId !== undefined ? (
              <EnumEditor
                value={value as FieldValue & { kind: "Enum" }}
                sessionId={sessionId}
                onCommit={v => { onEdit(v); setEditing(false); }}
                onCancel={() => setEditing(false)}
              />
            ) : (
              <InlineEditor
                value={value}
                onCommit={v => { onEdit(v); setEditing(false); }}
                onCancel={() => setEditing(false)}
              />
            )}
          </div>
        </div>
      );
    }

    // For Ref values, show a navigate button when onRefClick is provided
    const refNavigate = isRef && onRefClick && value.kind === "Ref"
      ? () => onRefClick((value as FieldValue & { kind: "Ref" }).target_file, (value as FieldValue & { kind: "Ref" }).target_key)
      : undefined;

    // Bool: toggle directly without opening an inline text editor
    const handleClick = canEdit
      ? value.kind === "Bool"
        ? () => onEdit({ kind: "Bool", v: !value.v })
        : () => setEditing(true)
      : undefined;

    return (
      <div
        style={{ marginLeft, display: "flex", alignItems: "center", gap: 6, padding: "2px 0", cursor: canEdit ? "pointer" : "default" }}
        onClick={handleClick}
        title={canEdit ? (value.kind === "Bool" ? "Click to toggle" : "Click to edit") : undefined}
      >
        {label && <span style={{ color: "var(--text-muted)", minWidth: 80, fontSize: 12 }}>{label}:</span>}
        {renderCompact(value)}
        {canEdit && <span style={{ color: "var(--text-muted)", fontSize: 10, opacity: 0.5 }}>✎</span>}
        {refNavigate && (
          <span
            onClick={e => { e.stopPropagation(); refNavigate(); }}
            title="跳转到引用记录"
            style={{ color: "var(--accent)", fontSize: 11, cursor: "pointer", padding: "0 2px" }}
          >↗</span>
        )}
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
            sessionId={sessionId}
            label={field.name}
            onRefClick={onRefClick}
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
    const defaultItem = (): FieldValue => {
      if (value.items.length > 0) {
        const first = value.items[0];
        switch (first.kind) {
          case "Bool": return { kind: "Bool", v: false };
          case "Int": return { kind: "Int", v: 0 };
          case "Float": return { kind: "Float", v: 0.0 };
          case "Str": return { kind: "Str", v: "" };
          case "Enum": return { kind: "Enum", enum_name: first.enum_name, variant: first.variant, int_value: first.int_value };
          case "Ref": return { kind: "Ref", target_type: first.target_type, target_key: "", target_file: first.target_file };
          case "Object": return { kind: "Object", actual_type: first.actual_type, fields: [] };
          default: return { kind: "Null" };
        }
      }
      return { kind: "Null" };
    };

    return (
      <div style={{ marginLeft }}>
        <div
          style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0", cursor: "pointer", userSelect: "none" }}
          onClick={() => setCollapsed(c => !c)}
        >
          {label && <span style={{ color: "var(--text-muted)", minWidth: 80, fontSize: 12 }}>{label}:</span>}
          <span style={{ color: "var(--text-muted)", fontSize: 10 }}>{collapsed ? "▶" : "▼"}</span>
          <span style={{ color: "var(--text-muted)" }}>[{value.items.length} items]</span>
          {onEdit && (
            <span
              onClick={e => { e.stopPropagation(); onEdit({ kind: "Array", items: [...value.items, defaultItem()] }); }}
              style={{ color: "var(--accent)", fontSize: 11, cursor: "pointer", marginLeft: 4 }}
              title="Add item"
            >＋</span>
          )}
        </div>
        {!collapsed && value.items.map((item, idx) => (
          <div key={idx} style={{ display: "flex", alignItems: "flex-start" }}>
            <div style={{ flex: 1 }}>
              <ExpandedValue
                value={item}
                depth={depth + 1}
                sessionId={sessionId}
                label={String(idx)}
                onRefClick={onRefClick}
                onEdit={onEdit ? (nv) => {
                  const newItems = [...value.items];
                  newItems[idx] = nv;
                  onEdit({ kind: "Array", items: newItems });
                } : undefined}
              />
            </div>
            {onEdit && (
              <span
                onClick={() => {
                  const newItems = value.items.filter((_, i) => i !== idx);
                  onEdit({ kind: "Array", items: newItems });
                }}
                style={{ color: "var(--text-muted)", fontSize: 11, cursor: "pointer", padding: "4px 4px", flexShrink: 0 }}
                title="Remove item"
              >×</span>
            )}
          </div>
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
          {onEdit && (
            <span
              onClick={e => {
                e.stopPropagation();
                // Infer key type from existing entries; default to Str for empty dict
                const defaultKey: DictKey = value.entries.length > 0 ? (() => {
                  const fk = value.entries[0].key;
                  switch (fk.kind) {
                    case "Int": return { kind: "Int", v: 0 };
                    case "Enum": return { kind: "Enum", enum_name: fk.enum_name, variant: fk.variant, int_value: fk.int_value };
                    default: return { kind: "Str", v: "" };
                  }
                })() : { kind: "Str", v: "" };
                const defaultVal: FieldValue = value.entries.length > 0 ? (() => {
                  const fv = value.entries[0].value;
                  switch (fv.kind) {
                    case "Bool": return { kind: "Bool", v: false };
                    case "Int": return { kind: "Int", v: 0 };
                    case "Float": return { kind: "Float", v: 0.0 };
                    case "Str": return { kind: "Str", v: "" };
                    case "Enum": return { kind: "Enum", enum_name: fv.enum_name, variant: fv.variant, int_value: fv.int_value };
                    case "Ref": return { kind: "Ref", target_type: fv.target_type, target_key: "", target_file: fv.target_file };
                    default: return { kind: "Null" };
                  }
                })() : { kind: "Null" };
                onEdit({ kind: "Dict", entries: [...value.entries, { key: defaultKey, value: defaultVal }] });
              }}
              style={{ color: "var(--accent)", fontSize: 11, cursor: "pointer", marginLeft: 4 }}
              title="Add entry"
            >＋</span>
          )}
        </div>
        {!collapsed && value.entries.map((entry, idx) => (
          <DictEntry
            key={idx}
            entry={entry}
            depth={depth + 1}
            sessionId={sessionId}
            onRefClick={onRefClick}
            onEditValue={onEdit ? (nv) => {
              const newEntries = [...value.entries];
              newEntries[idx] = { ...entry, value: nv };
              onEdit({ kind: "Dict", entries: newEntries });
            } : undefined}
            onEditKey={onEdit && (entry.key.kind === "Str" || entry.key.kind === "Int") ? (newKey) => {
              const newEntries = [...value.entries];
              if (entry.key.kind === "Int") {
                const n = parseInt(newKey, 10);
                newEntries[idx] = { ...entry, key: { kind: "Int", v: isNaN(n) ? 0 : n } };
              } else {
                newEntries[idx] = { ...entry, key: { kind: "Str", v: newKey } };
              }
              onEdit({ kind: "Dict", entries: newEntries });
            } : undefined}
            onRemove={onEdit ? () => {
              const newEntries = value.entries.filter((_, i) => i !== idx);
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
  // Prefer non-null fields first so collapsed nodes show meaningful data
  const nonNull = fields.filter(f => f.value.kind !== "Null");
  const base = nonNull.length > 0 ? nonNull : fields;
  const shown = base.slice(0, 4);
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
  sessionId?: number;
  onEdit?: (newValue: FieldValue) => void;
  onRefClick?: (targetFile: string | null, targetKey: string) => void;
  label?: string;
}

export function DataCard({ value, mode, depth = 0, sessionId, onEdit, onRefClick, label }: DataCardProps) {
  if (mode === "compact") {
    return <span style={{ fontFamily: "monospace", fontSize: 12 }}>{renderCompact(value)}</span>;
  }

  if (mode === "node") {
    return <NodeModeValue value={value} />;
  }

  // expanded
  return (
    <div style={{ fontFamily: "monospace", fontSize: 12 }}>
      <ExpandedValue value={value} depth={depth} sessionId={sessionId} onEdit={onEdit} onRefClick={onRefClick} label={label} />
    </div>
  );
}
