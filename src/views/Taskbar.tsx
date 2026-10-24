import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ThunderboltFilled } from "@ant-design/icons";
import { api, onSnapshot } from "../api";
import { deviceIcon } from "../icons";
import type { AppSettings, DeviceSnapshot, SnapshotEvent } from "../types";
import { levelColor, resolveDark } from "../theme";

interface Props {
  snapshot: SnapshotEvent | null;
  settings: AppSettings;
}

/**
 * 按配置挑选要显示的设备。
 * - 配了设备清单：只显示清单里、且没被隐藏的设备
 * - 清单里的设备一台都拿不到（被隐藏/拔掉）：退回显示所有读到电量的设备
 * - 全都读不到电量：至少把前几台设备名显示出来，不要一直"等待电量…"
 */
export function pickDevices(
  devices: DeviceSnapshot[],
  wanted: string[],
): DeviceSnapshot[] {
  const visible = devices.filter((d) => !d.hidden);
  const withReading = visible.filter((d) => d.reading?.percent != null);
  let list = wanted.length
    ? visible.filter((d) => wanted.includes(d.device.key))
    : withReading;
  if (list.length === 0) list = withReading;
  if (list.length === 0) list = visible.slice(0, 3);
  const order: Record<string, number> = { mouse: 0, keyboard: 1, combo: 2, headset: 3 };
  return [...list].sort(
    (a, b) => (order[a.device.kind] ?? 9) - (order[b.device.kind] ?? 9),
  );
}

export function shortName(name: string) {
  return (
    name
      .replace(/\s*(receiver|无线接收器|接收器)\s*/i, "")
      .replace(/\s*\(.*?\)\s*/g, "")
      .trim() || name
  );
}

export function Taskbar({ snapshot: outer, settings }: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  // 任务栏窗口是自己创建出来的，可能错过挂载前的事件广播，因此这里自持一份快照
  const [own, setOwn] = useState<SnapshotEvent | null>(null);

  useEffect(() => {
    let alive = true;
    // 窗口创建得比引擎首轮扫描早，快照可能还是空的 —— 轮询到真的读到电量为止
    (async () => {
      for (let i = 0; i < 40 && alive; i++) {
        try {
          const s = await api.snapshot();
          if (!alive) return;
          setOwn(s);
          if (s && s.devices && s.devices.some((d) => d.reading && d.reading.percent != null)) {
            return;
          }
        } catch {
          /* 引擎尚未就绪，继续等 */
        }
        await new Promise((r) => setTimeout(r, 500));
      }
    })();
    const un = onSnapshot((s) => setOwn(s));
    return () => {
      alive = false;
      un();
    };
  }, []);

  // 取"更新的一份"：自己轮询到的快照只用于窗口刚建好、还没收到广播时的占位；
  // 一旦主窗口把更新的快照推下来（outer），必须立刻改用 outer。
  // 以前写的是 `own ?? outer`——own 一旦被首轮轮询赋值就不再为 null，
  // outer 的后续更新被永久屏蔽，药丸电量就永远停在打开那一刻的数字上。
  const snapshot = useMemo(() => {
    if (!own) return outer;
    if (!outer) return own;
    const a = outer.refreshedAt ?? 0;
    const b = own.refreshedAt ?? 0;
    return a >= b ? outer : own;
  }, [own, outer]);
  const devices = pickDevices(snapshot?.devices ?? [], settings.taskbarDevices ?? []);
  const allHidden =
    (snapshot?.devices ?? []).length > 0 &&
    (snapshot?.devices ?? []).every((d) => d.hidden);
  const dark = resolveDark(settings.theme);

  // 内容宽度变化后通知后端调整窗口宽度：多测几次，直到设备都渲染出来
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    let cancelled = false;
    let tries = 0;

    const measure = () => {
      if (cancelled) return false;
      const style = window.getComputedStyle(el);
      const padL = parseFloat(style.paddingLeft) || 0;
      const padR = parseFloat(style.paddingRight) || 0;
      const gap = parseFloat(style.gap) || 0;
      let content = 0;
      let count = 0;
      el.querySelectorAll<HTMLElement>(":scope > span").forEach((n) => {
        const r = n.getBoundingClientRect();
        if (r.width > 0) {
          content += r.width + gap;
          count++;
        }
      });
      if (count > 0 && content > 40) {
        const w = Math.ceil(content + padL + padR - gap);
        if (Math.abs(w - (settings.taskbarWidth ?? 0)) > 4) {
          void api.resizeTaskbar(w).catch(() => undefined);
        }
        return true;
      }
      return false;
    };

    const tick = () => {
      if (cancelled) return;
      const done = measure();
      tries++;
      if (!done && tries < 20) window.setTimeout(tick, 250);
    };
    tick();

    return () => {
      cancelled = true;
    };
  }, [
    devices.map((d) => d.device.key + ":" + (d.reading?.percent ?? "-")).join(","),
    settings.taskbarWidth,
    settings.taskbarSize,
  ]);

  const size = settings.taskbarSize ?? "normal";
  const scale = size === "small" ? 0.9 : size === "large" ? 1.12 : 1;

  return (
    <div
      ref={rootRef}
      title={
        devices.length
          ? devices
              .map((d) => shortName(d.device.name) + " " + (d.reading?.percent ?? "—") + "%")
              .join("  ·  ")
          : allHidden
            ? "设备已在设置里隐藏"
            : "未检测到设备"
      }
      style={{
        height: "100%",
        display: "inline-flex",
        alignItems: "center",
        // 只显示内容：无底、无边框，内容右对齐贴住托盘一侧
        justifyContent: "flex-end",
        gap: 6 * scale,
        padding: "0 2px",
        background: "transparent",
        border: "none",
        borderRadius: 0,
        color: dark ? "#eef2f8" : "#1b2430",
        textShadow: dark
          ? "0 1px 2px rgba(0,0,0,0.55)"
          : "0 1px 2px rgba(255,255,255,0.75)",
        fontSize: 12 * scale,
        whiteSpace: "nowrap",
        overflow: "hidden",
        userSelect: "none",
        fontVariantNumeric: "tabular-nums",
        fontFamily: '"Segoe UI","Microsoft YaHei UI",system-ui,sans-serif',
      }}
    >
      {devices.length === 0 && (
        <span style={{ opacity: 0.6, fontSize: 11.5 * scale }}>
          {allHidden ? "设备已隐藏" : "等待电量…"}
        </span>
      )}
      {devices.map((snap, i) => {
        const p = snap.reading?.percent ?? null;
        const color = levelColor(p);
        return (
          <span
            key={snap.device.key}
            style={{ display: "inline-flex", alignItems: "center", gap: 3 * scale, flex: "none" }}
          >
            {i > 0 && (
              <span
                style={{
                  width: 1,
                  height: 11 * scale,
                  margin: "0 " + 4 * scale + "px",
                  background: dark ? "rgba(255,255,255,0.18)" : "rgba(20,30,60,0.18)",
                }}
              />
            )}
            <span style={{ fontSize: 11 * scale, opacity: 0.85 }}>
              {deviceIcon(snap.device.kind)}
            </span>
            <span
              style={{
                maxWidth: 92 * scale,
                overflow: "hidden",
                textOverflow: "ellipsis",
                fontSize: 11.5 * scale,
              }}
            >
              {shortName(snap.device.name)}
            </span>
            <span style={{ fontWeight: 650, color, fontSize: 12 * scale }}>
              {p == null ? "—" : p}
              {p == null ? "" : <small style={{ fontSize: 9 * scale, opacity: 0.75 }}>%</small>}
            </span>
            {snap.reading?.state === "charging" && (
              <ThunderboltFilled style={{ fontSize: 9 * scale, color: "#fbbf24" }} />
            )}
          </span>
        );
      })}
    </div>
  );
}
