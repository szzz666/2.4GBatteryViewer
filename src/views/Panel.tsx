import { useMemo, useState } from "react";
import { Badge, Button, Drawer, Empty, Flex, Space, Tooltip, Typography } from "antd";
import {
  ReloadOutlined,
  SettingOutlined,
  BorderOutlined,
  LaptopOutlined,
  CloudSyncOutlined,
  EyeInvisibleOutlined,
  CheckCircleFilled,
  LoadingOutlined,
  BugOutlined,
  ThunderboltFilled,
} from "@ant-design/icons";
import type { AppSettings, SnapshotEvent } from "../types";
import { BatteryCard } from "../components/BatteryCard";
import { SettingsPanel } from "../components/Settings";
import type { SaveState } from "../App";

const { Text } = Typography;

interface Props {
  snapshot: SnapshotEvent | null;
  settings: AppSettings;
  saveState: SaveState;
  onPatch: (patch: Partial<AppSettings>) => void;
  onRefresh: () => void;
  onOpenDebug: () => void;
  onRescan: () => void;
  onFlushSave: () => void;
  onResetSettings: () => void;
}

const KIND_ORDER: Record<string, number> = {
  mouse: 0,
  keyboard: 1,
  combo: 2,
  headset: 3,
  receiver: 4,
  other: 5,
};

export function Panel({
  snapshot,
  settings,
  saveState,
  onPatch,
  onRefresh,
  onOpenDebug,
  onRescan,
  onFlushSave,
  onResetSettings,
}: Props) {
  const [showSettings, setShowSettings] = useState(false);
  const all = snapshot?.devices ?? [];
  const devices = all.filter((d) => !d.hidden);
  const hiddenDevices = all.filter((d) => d.hidden);

  const sorted = useMemo(
    () =>
      [...devices].sort(
        (a, b) => (KIND_ORDER[a.device.kind] ?? 9) - (KIND_ORDER[b.device.kind] ?? 9),
      ),
    [devices],
  );

  const avg = useMemo(() => {
    const vals = sorted.map((d) => d.reading?.percent).filter((v): v is number => v != null);
    if (!vals.length) return null;
    return Math.round(vals.reduce((a, b) => a + b, 0) / vals.length);
  }, [sorted]);

  const restore = (key: string) => onPatch({ hidden: settings.hidden.filter((k) => k !== key) });

  return (
    <Flex vertical style={{ height: "100%", minHeight: 0, overflow: "hidden" }}>
      {/* 顶栏 */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
          padding: "14px 16px 12px",
          borderBottom: "1px solid var(--ant-color-border-secondary)",
          flex: "none",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, flex: "none" }}>
          <span
            style={{
              width: 36,
              height: 36,
              flex: "none",
              borderRadius: 10,
              display: "grid",
              placeItems: "center",
              background: "linear-gradient(135deg,#4f9cf9,#7c5cff)",
              color: "#fff",
              fontSize: 18,
            }}
          >
            <ThunderboltFilled />
          </span>
          <div>
            <div style={{ fontSize: 15, fontWeight: 600, lineHeight: 1.35, color: "var(--ant-color-text)", whiteSpace: "nowrap" }}>
              2.4G 外设电量
            </div>
            <div style={{ fontSize: 11, lineHeight: 1.35, color: "var(--ant-color-text-tertiary)", whiteSpace: "nowrap" }}>
              {sorted.length} 台设备
              {avg !== null ? " · 平均 " + avg + "%" : ""}
              {snapshot?.scanning ? " · 扫描中…" : ""}
            </div>
          </div>
        </div>

        <div style={{ display: "flex", gap: 6, flex: "none" }}>
          <Tooltip title="立即刷新（会重新探测所有协议）">
            <Button shape="circle" icon={<ReloadOutlined />} onClick={onRefresh} />
          </Tooltip>
          <Tooltip title={settings.taskbarEnabled ? "关闭任务栏显示" : "显示到任务栏"}>
            <Button
              shape="circle"
              type={settings.taskbarEnabled ? "primary" : "default"}
              icon={<BorderOutlined />}
              onClick={() => onPatch({ taskbarEnabled: !settings.taskbarEnabled })}
            />
          </Tooltip>
          <Tooltip title={settings.floatVisible ? "关闭悬浮框" : "显示悬浮框"}>
            <Button
              shape="circle"
              type={settings.floatVisible ? "primary" : "default"}
              icon={<LaptopOutlined />}
              onClick={() => onPatch({ floatVisible: !settings.floatVisible })}
            />
          </Tooltip>
          <Tooltip title="完整调试台">
            <Button shape="circle" icon={<BugOutlined />} onClick={onOpenDebug} />
          </Tooltip>
        </div>
      </div>

      {/* 设备列表 */}
      <div style={{ flex: "1 1 0", minHeight: 0, overflowY: "auto", overflowX: "hidden", padding: "12px 16px 16px" }}>
        {sorted.length === 0 ? (
          <Empty
            style={{ marginTop: 50 }}
            description={
              hiddenDevices.length > 0
                ? "所有设备都被隐藏了（" + hiddenDevices.length + " 台）"
                : "没有检测到支持的外设"
            }
          >
            {hiddenDevices.length > 0 ? (
              <Button type="primary" onClick={() => onPatch({ hidden: [] })}>
                全部恢复显示
              </Button>
            ) : (
              <Button type="primary" icon={<CloudSyncOutlined />} onClick={onRescan}>
                重新扫描
              </Button>
            )}
          </Empty>
        ) : (
          sorted.map((snap) => (
            <BatteryCard key={snap.device.key} snap={snap} onOpenDebug={onOpenDebug} />
          ))
        )}

        {hiddenDevices.length > 0 && (
          <div
            style={{
              marginTop: 8,
              padding: "10px 12px",
              borderRadius: 10,
              border: "1px dashed var(--ant-color-border)",
            }}
          >
            <Space size={6} style={{ marginBottom: 8 }}>
              <EyeInvisibleOutlined style={{ color: "var(--ant-color-text-tertiary)" }} />
              <Text type="secondary" style={{ fontSize: 12 }}>
                已隐藏 {hiddenDevices.length} 台设备
              </Text>
              <Typography.Link style={{ fontSize: 12 }} onClick={() => onPatch({ hidden: [] })}>
                全部恢复
              </Typography.Link>
            </Space>
            <Flex vertical gap={4}>
              {hiddenDevices.map((d) => (
                <Flex key={d.device.key} align="center" justify="space-between" gap={8}>
                  <Text type="secondary" ellipsis style={{ fontSize: 12 }}>
                    {d.device.name}
                  </Text>
                  <Typography.Link style={{ fontSize: 12, whiteSpace: "nowrap" }} onClick={() => restore(d.device.key)}>
                    恢复显示
                  </Typography.Link>
                </Flex>
              ))}
            </Flex>
          </div>
        )}
      </div>

      {/* 底栏 */}
      <Flex
        align="center"
        justify="space-between"
        style={{
          padding: "8px 16px",
          borderTop: "1px solid var(--ant-color-border-secondary)",
          flex: "none",
        }}
      >
        <Space size={6}>
          <Badge status="processing" />
          <Text type="secondary" style={{ fontSize: 11 }}>
            每 {Math.round(settings.refreshMs / 1000)} 秒刷新
          </Text>
          {saveState === "saved" && (
            <Text type="success" style={{ fontSize: 11 }}>
              <CheckCircleFilled /> 设置已保存
            </Text>
          )}
          {saveState === "saving" && (
            <Text type="secondary" style={{ fontSize: 11 }}>
              <LoadingOutlined /> 保存中
            </Text>
          )}
        </Space>
        <Button type="link" size="small" icon={<SettingOutlined />} onClick={() => setShowSettings(true)}>
          设置
        </Button>
      </Flex>

      <Drawer
        title="设置"
        placement="right"
        width={368}
        open={showSettings}
        onClose={() => {
          setShowSettings(false);
          onFlushSave();
        }}
        styles={{ body: { paddingTop: 8 } }}
        footer={
          <Flex justify="space-between" align="center">
            <Button danger type="text" size="small" onClick={onResetSettings}>
              恢复默认
            </Button>
            <Button
              type="primary"
              onClick={() => {
                onFlushSave();
                setShowSettings(false);
              }}
            >
              完成
            </Button>
          </Flex>
        }
      >
        <SettingsPanel
          settings={settings}
          devices={all}
          saveState={saveState}
          onChange={onPatch}
          onClose={() => {
            setShowSettings(false);
            onFlushSave();
          }}
        />
      </Drawer>
    </Flex>
  );
}
