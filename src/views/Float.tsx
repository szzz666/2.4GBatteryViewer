import { useEffect, useLayoutEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button, Space } from "antd";
import {
  LockFilled,
  UnlockOutlined,
  ExpandAltOutlined,
  CloseOutlined,
  ThunderboltFilled,
} from "@ant-design/icons";
import { deviceIcon } from "../icons";
import { api } from "../api";
import type { AppSettings, SnapshotEvent } from "../types";
import { levelColor, resolveDark } from "../theme";
import { pickDevices, shortName } from "./Taskbar";

interface Props {
  snapshot: SnapshotEvent | null;
  settings: AppSettings;
  onPatch: (patch: Partial<AppSettings>) => void;
  onOpenPanel: () => void;
}

export function Float({ snapshot, settings, onPatch, onOpenPanel }: Props) {
  const locked = settings.floatLocked;
  const dragging = useRef(false);
  const moveTimer = useRef<number | undefined>(undefined);
  const rootRef = useRef<HTMLDivElement>(null);

  const zoom = Math.min(1.8, Math.max(0.8, settings.floatZoom ?? 1));
  const opacity = Math.min(1, Math.max(0.15, settings.floatOpacity ?? 0.72));
  const dark = resolveDark(settings.theme);
  const devices = pickDevices(snapshot?.devices ?? [], settings.floatDevices ?? []);
  const allHidden =
    (snapshot?.devices ?? []).length > 0 && (snapshot?.devices ?? []).every((d) => d.hidden);

  // 单行显示，窗口尺寸跟着内容走
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const id = window.setTimeout(() => {
      const style = window.getComputedStyle(el);
      const padL = parseFloat(style.paddingLeft) || 0;
      const padR = parseFloat(style.paddingRight) || 0;
      let content = 0;
      el.querySelectorAll<HTMLElement>(":scope > span, :scope > div").forEach((n) => {
        const r = n.getBoundingClientRect();
        if (r.width > 0) content += r.width + (parseFloat(style.gap) || 0);
      });
      const w = Math.ceil(content + padL + padR);
      const h = Math.ceil(el.getBoundingClientRect().height);
      // 尺寸没变就不要调用，避免 set_size 触发 onMoved 把位置写乱
      const curW = Math.round(el.getBoundingClientRect().width);
      if (w > 80 && h > 20 && Math.abs(w - curW) > 2) {
        void api.resizeFloat(w, h).catch(() => undefined);
      }
    }, 160);
    return () => window.clearTimeout(id);
  }, [devices.map((d) => d.device.key + ":" + (d.reading?.percent ?? "-")).join(","), zoom, locked]);

  useEffect(() => {
    let un: (() => void) | undefined;
    (async () => {
      try {
        const win = getCurrentWindow();
        un = await win.onMoved(({ payload }) => {
          if (locked || dragging.current) return;
          // 拖动时每秒几十次回调，节流后再落盘
          if (moveTimer.current) window.clearTimeout(moveTimer.current);
          moveTimer.current = window.setTimeout(() => {
            void api.saveFloatPosition(payload.x, payload.y).catch(() => undefined);
          }, 400);
        });
      } catch {
        /* ignore */
      }
    })();
    return () => {
      if (moveTimer.current) window.clearTimeout(moveTimer.current);
      un?.();
    };
  }, [locked]);

  const startDrag = async (e: React.MouseEvent) => {
    if (locked) return;
    if ((e.target as HTMLElement).closest("button")) return;
    dragging.current = true;
    try {
      await api.startDrag();
    } catch {
      /* ignore */
    } finally {
      setTimeout(() => {
        dragging.current = false;
      }, 400);
    }
  };

  const toggleLock = async () => {
    const next = !locked;
    onPatch({ floatLocked: next });
    try {
      await api.setFloatLocked(next);
    } catch {
      /* ignore */
    }
  };

  const close = async () => {
    try {
      await api.closeFloat();
    } catch {
      /* ignore */
    }
    onPatch({ floatVisible: false });
  };

  return (
    <div
      ref={rootRef}
      onMouseDown={startDrag}
      style={{
        height: "100%",
        display: "inline-flex",
        alignItems: "center",
        gap: 6 * zoom,
        padding: "0 " + 10 * zoom + "px",
        borderRadius: 12,
        background: dark
          ? "rgba(20, 23, 30, " + opacity + ")"
          : "rgba(255, 255, 255, " + opacity + ")",
        border: "1px solid " + (dark ? "rgba(255,255,255,0.10)" : "rgba(20,30,60,0.12)"),
        backdropFilter: "blur(14px)",
        color: dark ? "#eef2f8" : "#131722",
        fontSize: 13 * zoom,
        whiteSpace: "nowrap",
        overflow: "hidden",
        cursor: locked ? "default" : "grab",
        userSelect: "none",
      }}
    >
      <span
        style={{
          width: 4,
          height: "45%",
          borderRadius: 2,
          flex: "none",
          background: dark ? "rgba(255,255,255,0.22)" : "rgba(20,30,60,0.2)",
        }}
      />

      {devices.length === 0 && (
        <span style={{ fontSize: 12 * zoom, opacity: 0.65 }}>
          {allHidden ? "设备已隐藏" : "等待电量…"}
        </span>
      )}

      {devices.map((snap, i) => {
        const p = snap.reading?.percent ?? null;
        const color = levelColor(p);
        return (
          <span
            key={snap.device.key}
            style={{ display: "inline-flex", alignItems: "center", gap: 4 * zoom, flex: "none" }}
          >
            {i > 0 && <span style={{ opacity: 0.35, margin: "0 " + 2 * zoom + "px" }}>·</span>}
            <span style={{ fontSize: 12 * zoom, opacity: 0.85 }}>
              {deviceIcon(snap.device.kind)}
            </span>
            <span
              style={{
                fontSize: 12 * zoom,
                maxWidth: 160 * zoom,
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {shortName(snap.device.name)}
            </span>
            <span
              style={{
                fontSize: 14 * zoom,
                fontWeight: 650,
                color,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {p == null ? "—" : p}
              {p == null ? "" : <small style={{ fontSize: 9 * zoom, opacity: 0.75, marginLeft: 1 }}>%</small>}
            </span>
            {snap.reading?.state === "charging" && (
              <ThunderboltFilled style={{ fontSize: 10 * zoom, color: "#fbbf24" }} />
            )}
          </span>
        );
      })}

      {/* 刻意不用 Tooltip：气泡会盖住按钮、吞掉点击 */}
      <Space size={2} style={{ marginLeft: 6 * zoom, flex: "none" }}>
        <Button
          type="text"
          size="small"
          style={{
            color: locked ? "#fbbf24" : undefined,
            width: 20 * zoom,
            height: 20 * zoom,
            minWidth: 20 * zoom,
          }}
          icon={locked ? <LockFilled /> : <UnlockOutlined />}
          onClick={() => void toggleLock()}
        />
        <Button
          type="text"
          size="small"
          style={{ width: 20 * zoom, height: 20 * zoom, minWidth: 20 * zoom }}
          icon={<ExpandAltOutlined />}
          onClick={onOpenPanel}
        />
        <Button
          type="text"
          size="small"
          style={{ width: 20 * zoom, height: 20 * zoom, minWidth: 20 * zoom }}
          icon={<CloseOutlined />}
          onClick={() => void close()}
        />
      </Space>
    </div>
  );
}
