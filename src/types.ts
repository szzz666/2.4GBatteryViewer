export type BatteryState = "discharging" | "charging" | "full" | "unknown";

export interface HidInterface {
  path: string;
  vendorId: number;
  productId: number;
  manufacturer: string | null;
  product: string | null;
  serial: string | null;
  usagePage: number;
  usage: number;
  interfaceNumber: number;
  inputLen: number;
  outputLen: number;
  featureLen: number;
  reportIds: number[];
  instanceId: string | null;
  busReportedDesc: string | null;
}

export interface DeviceInfo {
  key: string;
  vendorId: number;
  productId: number;
  name: string;
  manufacturer: string | null;
  kind: string;
  interfaces: HidInterface[];
}

export interface Reading {
  key: string;
  percent: number | null;
  state: BatteryState;
  voltageMv: number | null;
  source: string;
  protocol: string;
  note: string | null;
  updatedAt: number;
  latencyMs: number;
}

export interface DeviceSnapshot {
  device: DeviceInfo;
  reading: Reading | null;
  /** 被用户在设置里隐藏（仍会出现在列表里，方便恢复） */
  hidden: boolean;
}

export interface SnapshotEvent {
  devices: DeviceSnapshot[];
  scanning: boolean;
  refreshedAt: number;
  refreshMs: number;
}

export interface AppSettings {
  refreshMs: number;
  autostart: boolean;
  floatVisible: boolean;
  floatX: number;
  floatY: number;
  floatLocked: boolean;
  theme: string;
  overrides: Record<string, string>;
  hidden: string[];
  warnPercent: number;
  // 任务栏
  taskbarEnabled: boolean;
  taskbarWidth: number;
  taskbarDevices: string[];
  taskbarSize: string;
  // 悬浮框
  floatZoom: number;
  floatOpacity: number;
  floatDevices: string[];
}

export interface ProtocolMeta {
  id: string;
  name: string;
  description: string;
}

export interface FeatureResult {
  ok: boolean;
  data: number[];
  error: string | null;
  elapsedMs: number;
}

export interface ProbeResult {
  ok: boolean;
  sent: number[];
  received: number[][];
  error: string | null;
  elapsedMs: number;
}

export interface ReportIdProbe {
  reportId: number;
  featureLen: number;
  nonZero: boolean;
  data: number[];
}

export interface ProtocolTry {
  protocol: string;
  name: string;
  ok: boolean;
  percent: number | null;
  state: BatteryState;
  voltageMv: number | null;
  detail: string;
  raw: string[];
  elapsedMs: number;
}