import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppSettings,
  DeviceInfo,
  FeatureResult,
  HidInterface,
  ProbeResult,
  ProtocolMeta,
  ProtocolTry,
  ReportIdProbe,
  SnapshotEvent,
} from "./types";

export const api = {
  snapshot: () => invoke<SnapshotEvent>("get_snapshot"),
  rescan: () => invoke<SnapshotEvent>("rescan"),
  refreshNow: () => invoke<SnapshotEvent>("refresh_now"),
  settings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings }),
  resetSettings: () => invoke<AppSettings>("reset_settings"),
  devices: () => invoke<DeviceInfo[]>("list_devices"),
  enumerateAll: () => invoke<DeviceInfo[]>("enumerate_all"),
  protocols: () => invoke<ProtocolMeta[]>("list_protocols"),
  aulaDiagnose: (path: string) => invoke<any>("aula_diagnose", { path }),
  clearOverride: (key: string) => invoke<void>("clear_override", { key }),
  setTaskbarEnabled: (enabled: boolean) => invoke<void>("set_taskbar_enabled", { enabled }),
  resizeTaskbar: (width: number) => invoke<void>("resize_taskbar", { width }),
  resizeFloat: (width: number, height: number) => invoke<void>("resize_float", { width, height }),
  readFeature: (path: string, reportId: number, length: number, useFeature = true) =>
    invoke<FeatureResult>("read_feature", { path, reportId, length, useFeature }),
  writeFeature: (path: string, hex: string) =>
    invoke<FeatureResult>("write_feature", { path, hex }),
  sendProbe: (path: string, hex: string, timeoutMs = 500, asFeature = false) =>
    invoke<ProbeResult>("send_probe", { path, hex, timeoutMs, asFeature }),
  probeReportIds: (path: string, featureLen: number) =>
    invoke<ReportIdProbe[]>("probe_report_ids", { path, featureLen }),
  diagnose: (key: string) => invoke<ProtocolTry[]>("diagnose_device", { key }),
  tryProtocol: (key: string, protocol: string, path?: string) =>
    invoke<ProtocolTry>("try_protocol", { key, protocol, path: path ?? null }),
  calibrate: (key: string, protocol: string, path?: string) =>
    invoke<ProtocolTry>("calibrate", { key, protocol, path: path ?? null }),
  readInput: (path: string, timeoutMs = 800) =>
    invoke<number[][]>("read_input", { path, timeoutMs }),
  setMode: (mode: string) => invoke<void>("set_mode", { mode }),
  getMode: () => invoke<string>("get_mode"),
  setFloatLocked: (locked: boolean) => invoke<void>("set_float_locked", { locked }),
  closeFloat: () => invoke<void>("close_float"),
  startDrag: () => invoke<void>("start_drag"),
  saveFloatPosition: (x: number, y: number) => invoke<void>("save_float_position", { x, y }),
  toggleAutostart: (enabled: boolean) => invoke<boolean>("toggle_autostart", { enabled }),
  isAutostartEnabled: () => invoke<boolean>("is_autostart_enabled"),
  hideWindow: (label: string) => invoke<void>("hide_window", { label }),
  showWindow: (label: string) => invoke<void>("show_window", { label }),
  quit: () => invoke<void>("quit_app"),
  interfaces: () => invoke<HidInterface[]>("list_interfaces"),
};

/**
 * 注册事件监听，失败就重试。
 *
 * 任务栏/悬浮框是在窗口隐藏状态下创建的，这时 listen() 的内部 eval 可能失败，
 * 注册不上监听器而且不会报错——表现就是"改了设置，小窗纹丝不动"。
 * 这里退避重试直到注册成功，同时各小窗还有轮询兜底。
 */
export function listenRetry<T>(
  event: string,
  cb: (payload: T) => void,
  tries = 40,
  delay = 400,
): () => void {
  let disposed = false;
  let un: (() => void) | undefined;
  (async () => {
    for (let i = 0; i < tries && !disposed; i++) {
      try {
        un = await listen<T>(event, (e) => cb(e.payload));
        if (disposed) un();
        return;
      } catch {
        await new Promise((r) => setTimeout(r, delay));
      }
    }
  })();
  return () => {
    disposed = true;
    un?.();
  };
}

export function onSnapshot(cb: (s: SnapshotEvent) => void) {
  return listenRetry<SnapshotEvent>("battery://snapshot", cb);
}

/** 任意窗口改了设置，其余窗口跟着更新（否则悬浮框/任务栏会拿着旧设置不吭声） */
export function onSettings(cb: (s: AppSettings) => void) {
  return listenRetry<AppSettings>("settings://changed", cb);
}

export function onLowBattery(cb: (p: { key: string; name: string; percent: number }) => void) {
  return listenRetry<{ key: string; name: string; percent: number }>("battery://low", cb);
}

export function hex(bytes: number[] | Uint8Array | null | undefined): string {
  if (!bytes || bytes.length === 0) return "";
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0").toUpperCase())
    .join(" ");
}

export function parseHexInput(text: string): number[] {
  const clean = text.replace(/0x/gi, " ").replace(/[^0-9a-fA-F]/g, "");
  const out: number[] = [];
  for (let i = 0; i + 1 < clean.length; i += 2) {
    out.push(parseInt(clean.slice(i, i + 2), 16));
  }
  if (clean.length % 2 === 1) out.push(parseInt(clean + "0", 16));
  return out;
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
