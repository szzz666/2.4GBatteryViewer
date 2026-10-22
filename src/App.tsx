import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ConfigProvider, App as AntApp } from "antd";
import { ThunderboltFilled } from "@ant-design/icons";
import zhCN from "antd/locale/zh_CN";
import { api, isTauri, onLowBattery, onSettings, onSnapshot } from "./api";
import type { AppSettings, SnapshotEvent } from "./types";
import { Panel } from "./views/Panel";
import { Float } from "./views/Float";
import { Debug } from "./views/Debug";
import { Taskbar } from "./views/Taskbar";
import { buildTheme, resolveDark } from "./theme";
import "./App.css";

type View = "panel" | "float" | "debug" | "taskbar";
export type SaveState = "idle" | "saving" | "saved" | "error";

function detectView(): View {
  if (typeof window === "undefined") return "panel";
  if (window.location.search.includes("view=float")) return "float";
  if (window.location.search.includes("view=debug")) return "debug";
  if (window.location.search.includes("view=taskbar")) return "taskbar";
  const label = document.documentElement.dataset.windowLabel ?? "";
  if (label === "float") return "float";
  if (label === "debug") return "debug";
  if (label === "taskbar") return "taskbar";
  return "panel";
}

const DEFAULT_SETTINGS: AppSettings = {
  refreshMs: 30000,
  autostart: false,
  floatVisible: false,
  floatX: -1,
  floatY: -1,
  floatLocked: false,
  theme: "auto",
  overrides: {},
  hidden: [],
  warnPercent: 15,
  taskbarEnabled: false,
  taskbarWidth: 240,
  taskbarDevices: [],
  taskbarSize: "normal",
  floatZoom: 1,
  floatOpacity: 0.72,
  floatDevices: [],
};

export default function App() {
  const [view] = useState<View>(detectView);
  const [snapshot, setSnapshot] = useState<SnapshotEvent | null>(null);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [error, setError] = useState<string | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [lowMsg, setLowMsg] = useState<string | null>(null);

  // 拖滑块会连发几十次 onChange，节流成一次落盘，避免写坏文件 / 打满 HID 线程
  const settingsRef = useRef(settings);
  const saveTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const [snap, cfg] = await Promise.all([api.snapshot(), api.settings()]);
        if (!alive) return;
        setSnapshot(snap);
        settingsRef.current = cfg;
        setSettings(cfg);
      } catch (e) {
        setError(String(e));
      }
    })();
    const un = onSnapshot((s) => setSnapshot(s));
    const unCfg = onSettings((s) => {
      settingsRef.current = s;
      setSettings(s);
    });
    const unLow = onLowBattery((p) => {
      setLowMsg(p.name + " 电量仅剩 " + p.percent + "%");
      window.setTimeout(() => setLowMsg(null), 8000);
    });
    return () => {
      alive = false;
      un();
      unCfg();
      unLow();
    };
  }, []);

  // 悬浮框 / 任务栏是独立小窗，事件通道偶尔注册不上：再加一条 2 秒轮询兜底，
  // 保证电量、设置永远是最新的（两次 IPC 调用，开销可以忽略）。
  useEffect(() => {
    if (view !== "float" && view !== "taskbar") return;
    let busy = false;
    const tick = async () => {
      if (busy) return;
      busy = true;
      try {
        const [snap, cfg] = await Promise.all([api.snapshot(), api.settings()]);
        setSnapshot(snap);
        const prev = settingsRef.current;
        if (
          !prev ||
          cfg.theme !== prev.theme ||
          cfg.taskbarSize !== prev.taskbarSize ||
          cfg.taskbarDevices.join() !== prev.taskbarDevices.join() ||
          cfg.floatDevices.join() !== prev.floatDevices.join() ||
          cfg.floatZoom !== prev.floatZoom ||
          cfg.floatOpacity !== prev.floatOpacity ||
          cfg.hidden.join() !== prev.hidden.join() ||
          cfg.taskbarEnabled !== prev.taskbarEnabled ||
          cfg.floatVisible !== prev.floatVisible ||
          cfg.floatLocked !== prev.floatLocked ||
          cfg.warnPercent !== prev.warnPercent ||
          cfg.refreshMs !== prev.refreshMs ||
          cfg.taskbarWidth !== prev.taskbarWidth
        ) {
          settingsRef.current = cfg;
          setSettings(cfg);
        }
      } catch {
        /* 忽略瞬时错误，下一轮再来 */
      } finally {
        busy = false;
      }
    };
    const timer = window.setInterval(() => void tick(), 2000);
    return () => window.clearInterval(timer);
  }, [view]);

  const flushSave = useCallback(async () => {
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = undefined;
    }
    try {
      await api.saveSettings(settingsRef.current);
      setSaveState("saved");
    } catch (e) {
      setSaveState("error");
      setError("保存设置失败：" + String(e));
    }
  }, []);

  const scheduleSave = useCallback(() => {
    setSaveState("saving");
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      void flushSave();
    }, 300);
  }, [flushSave]);

  const patch = useCallback(
    (p: Partial<AppSettings>) => {
      setSettings((prev) => {
        const next = { ...prev, ...p };
        settingsRef.current = next;
        return next;
      });
      // 这两项要立刻动窗口，不能等节流
      if (p.taskbarEnabled !== undefined) {
        void api.setTaskbarEnabled(p.taskbarEnabled).catch((e) => setError(String(e)));
      }
      if (p.floatVisible !== undefined) {
        void (p.floatVisible ? api.setMode("floating") : api.closeFloat()).catch((e) =>
          setError(String(e)),
        );
      }
      scheduleSave();
    },
    [scheduleSave],
  );

  const resetSettings = useCallback(async () => {
    try {
      const s = await api.resetSettings();
      settingsRef.current = s;
      setSettings(s);
      setSaveState("saved");
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // 关闭窗口时把没落盘的改动写进去
  useEffect(() => {
    const onHide = () => {
      if (saveTimer.current) void flushSave();
    };
    window.addEventListener("beforeunload", onHide);
    return () => window.removeEventListener("beforeunload", onHide);
  }, [flushSave]);

  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      if (settings.theme === "auto") {
        const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        root.dataset.theme = dark ? "dark" : "light";
      } else {
        root.dataset.theme = settings.theme;
      }
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [settings.theme]);

  const refresh = useCallback(async () => {
    try {
      setSnapshot(await api.refreshNow());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const rescan = useCallback(async () => {
    try {
      setSnapshot(await api.rescan());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const openMode = useCallback(async (mode: string) => {
    try {
      await api.setMode(mode);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const body = useMemo(() => {
    if (!isTauri()) {
      return (
        <div style={{ padding: 40, textAlign: "center" }}>
          <h2>请在 Tauri 桌面应用中使用</h2>
          <p>当前是浏览器预览，无法访问本机 HID 设备。</p>
        </div>
      );
    }
    if (view === "float") {
      return (
        <Float
          snapshot={snapshot}
          settings={settings}
          onPatch={(p) => void patch(p)}
          onOpenPanel={() => void openMode("panel")}
        />
      );
    }
    if (view === "debug") {
      return <Debug />;
    }
    if (view === "taskbar") {
      return <Taskbar snapshot={snapshot} settings={settings} />;
    }
    return (
      <Panel
        snapshot={snapshot}
        settings={settings}
        saveState={saveState}
        onPatch={(p) => void patch(p)}
        onRefresh={() => void refresh()}
        onRescan={() => void rescan()}
        onOpenDebug={() => void openMode("debug")}
        onFlushSave={() => void flushSave()}
        onResetSettings={() => void resetSettings()}
      />
    );
  }, [view, snapshot, settings, saveState, patch, refresh, rescan, openMode, flushSave, resetSettings]);

  const dark = resolveDark(settings.theme);

  return (
    <ConfigProvider locale={zhCN} theme={buildTheme(dark)}>
      <AntApp>
        <div
          className={"app app-" + view}
          style={
            view === "float" || view === "taskbar"
              ? undefined
              : { background: "var(--ant-color-bg-container)", color: "var(--ant-color-text)" }
          }
        >
          {body}
          {error && (
            <div className="toast" onClick={() => setError(null)}>
              {error}
            </div>
          )}
          {lowMsg && (
            <div className="toast warn" onClick={() => setLowMsg(null)}>
              <ThunderboltFilled /> {lowMsg}
            </div>
          )}
        </div>
      </AntApp>
    </ConfigProvider>
  );
}
