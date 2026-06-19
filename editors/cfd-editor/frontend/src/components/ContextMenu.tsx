import { useEffect, useRef, useState } from "react";

export interface ContextMenuItem {
  label: string;
  onClick: () => void;
  danger?: boolean;
}

interface ContextMenuProps {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

export function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });

  useEffect(() => {
    const handleClick = () => onClose();
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose]);

  // Clamp to viewport after first render when we know the menu dimensions
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const clampedX = Math.min(x, vw - rect.width - 4);
    const clampedY = Math.min(y, vh - rect.height - 4);
    if (clampedX !== x || clampedY !== y) {
      setPos({ x: Math.max(4, clampedX), y: Math.max(4, clampedY) });
    }
  }, [x, y]);

  return (
    <div
      ref={ref}
      onMouseDown={e => e.stopPropagation()}
      style={{
        position: "fixed",
        top: pos.y,
        left: pos.x,
        background: "var(--bg2)",
        border: "1px solid var(--border)",
        borderRadius: 6,
        boxShadow: "0 4px 16px rgba(0,0,0,0.5)",
        minWidth: 160,
        zIndex: 1000,
        padding: "4px 0",
      }}
    >
      {items.map((item, idx) => (
        <div
          key={idx}
          onClick={() => { item.onClick(); onClose(); }}
          style={{
            padding: "6px 14px",
            cursor: "pointer",
            fontSize: 13,
            color: item.danger ? "var(--error)" : "var(--text)",
            userSelect: "none",
          }}
          onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
          onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
        >
          {item.label}
        </div>
      ))}
    </div>
  );
}

export interface ContextMenuState {
  x: number;
  y: number;
  items: ContextMenuItem[];
}
