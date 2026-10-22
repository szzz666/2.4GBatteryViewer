import { useEffect, useState } from "react";
import {
  Card,
  Divider,
  Flex,
  InputNumber,
  Segmented,
  Slider,
  Space,
  Switch,
  Typography,
} from "antd";
import { CheckCircleFilled, LoadingOutlined } from "@ant-design/icons";
import type { AppSettings, DeviceSnapshot } from "../types";
import { DevicePicker } from "./DevicePicker";
import type { SaveState } from "../App";

const { Text } = Typography;
const PRESETS = [5, 10, 30, 60, 300];

interface Props {
  settings: AppSettings;
  devices: DeviceSnapshot[];
  saveState?: SaveState;
  onChange: (patch: Partial<AppSettings>) => void;
  onClose: () => void;
}

function Section({
  title,
  extra,
  children,
}: {
  title: string;
  extra?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <Card
      size="small"
      title={<Text strong style={{ fontSize: 13 }}>{title}</Text>}
      extra={extra}
      style={{ marginBottom: 12 }}
      styles={{ body: { padding: 12 } }}
    >
      {children}
    </Card>
  );
}

export function SettingsPanel({ settings, devices, saveState, onChange }: Props) {
  const [custom, setCustom] = useState(Math.round(settings.refreshMs / 1000));
  useEffect(() => {
    setCustom(Math.round(settings.refreshMs / 1000));
  }, [settings.refreshMs]);

  return (
    <div>
      <Flex align="center" gap={6} style={{ marginBottom: 10 }}>
        <Text type="secondary" style={{ fontSize: 11 }}>
          改动立即生效并自动保存
        </Text>
        {saveState === "saved" && (
          <Text type="success" style={{ fontSize: 11 }}>
            <CheckCircleFilled /> 已保存
          </Text>
        )}
        {saveState === "saving" && (
          <Text type="secondary" style={{ fontSize: 11 }}>
            <LoadingOutlined /> 保存中
          </Text>
        )}
      </Flex>

      <Section title="自动刷新频率">
        <Segmented
          block
          value={PRESETS.includes(Math.round(settings.refreshMs / 1000)) ? Math.round(settings.refreshMs / 1000) : "自定义"}
          options={[
            ...PRESETS.map((s) => ({ label: s < 60 ? s + " 秒" : s / 60 + " 分钟", value: s })),
          ]}
          onChange={(v) => onChange({ refreshMs: Number(v) * 1000 })}
        />
        <Space style={{ marginTop: 10 }} size={8}>
          <InputNumber
            min={2}
            max={3600}
            value={custom}
            onChange={(v) => setCustom(Number(v ?? 30))}
            onBlur={() => {
              if (Number.isFinite(custom) && custom >= 2) onChange({ refreshMs: custom * 1000 });
            }}
            style={{ width: 96 }}
            addonAfter="秒"
          />
          <Text type="secondary" style={{ fontSize: 11 }}>
            2 秒 ~ 1 小时
          </Text>
        </Space>
      </Section>

      <Section
        title="任务栏显示"
        extra={
          <Switch
            size="small"
            checked={settings.taskbarEnabled}
            onChange={(v) => onChange({ taskbarEnabled: v })}
          />
        }
      >
        <Text type="secondary" style={{ fontSize: 11 }}>
          在任务栏右侧常驻一条电量小条。Explorer 重启后会自动贴回原位。
        </Text>
        {settings.taskbarEnabled && (
          <>
            <Divider style={{ margin: "10px 0" }} />
            <Text style={{ fontSize: 12 }}>字号</Text>
            <Segmented
              block
              style={{ marginTop: 6 }}
              value={settings.taskbarSize}
              options={[
                { label: "小", value: "small" },
                { label: "标准", value: "normal" },
                { label: "大", value: "large" },
              ]}
              onChange={(v) => onChange({ taskbarSize: String(v) })}
            />
            <Divider style={{ margin: "10px 0" }} />
            <Text style={{ fontSize: 12 }}>显示的设备</Text>
            <DevicePicker
              devices={devices}
              selected={settings.taskbarDevices}
              onChange={(keys) => onChange({ taskbarDevices: keys })}
            />
          </>
        )}
      </Section>

      <Section
        title="悬浮框"
        extra={
          <Switch
            size="small"
            checked={settings.floatVisible}
            onChange={(v) => onChange({ floatVisible: v })}
          />
        }
      >
        <Text type="secondary" style={{ fontSize: 11 }}>
          半透明单行小窗，可拖动；锁定后位置固定。上面两个按钮可以随时开关。
        </Text>
        {settings.floatVisible && (
          <>
            <Divider style={{ margin: "10px 0" }} />
            <Flex align="center" gap={8}>
              <Text type="secondary" style={{ fontSize: 12, width: 52 }}>
                大小
              </Text>
              <Slider
                style={{ flex: 1 }}
                min={80}
                max={180}
                step={5}
                value={Math.round((settings.floatZoom ?? 1) * 100)}
                onChange={(v) => onChange({ floatZoom: v / 100 })}
                tooltip={{ formatter: (v) => v + "%" }}
              />
              <Text className="mono" style={{ fontSize: 11, width: 38, textAlign: "right" }}>
                {Math.round((settings.floatZoom ?? 1) * 100)}%
              </Text>
            </Flex>
            <Flex align="center" gap={8}>
              <Text type="secondary" style={{ fontSize: 12, width: 52 }}>
                不透明度
              </Text>
              <Slider
                style={{ flex: 1 }}
                min={15}
                max={100}
                value={Math.round((settings.floatOpacity ?? 0.72) * 100)}
                onChange={(v) => onChange({ floatOpacity: v / 100 })}
                tooltip={{ formatter: (v) => v + "%" }}
              />
              <Text className="mono" style={{ fontSize: 11, width: 38, textAlign: "right" }}>
                {Math.round((settings.floatOpacity ?? 0.72) * 100)}%
              </Text>
            </Flex>
            <Divider style={{ margin: "10px 0" }} />
            <Text style={{ fontSize: 12 }}>显示的设备</Text>
            <DevicePicker
              devices={devices}
              selected={settings.floatDevices}
              onChange={(keys) => onChange({ floatDevices: keys })}
            />
          </>
        )}
      </Section>

      <Section
        title="开机自启"
        extra={
          <Switch
            size="small"
            checked={settings.autostart}
            onChange={(v) => onChange({ autostart: v })}
          />
        }
      >
        <Text type="secondary" style={{ fontSize: 11 }}>
          写入当前用户注册表 Run 项，无需管理员权限；启动后静默到托盘。
        </Text>
      </Section>

      <Section title={"低电量提醒 · " + settings.warnPercent + "%"}>
        <Slider
          min={5}
          max={50}
          value={settings.warnPercent}
          onChange={(v) => onChange({ warnPercent: v })}
          tooltip={{ formatter: (v) => v + "%" }}
        />
      </Section>

      <Section title="不显示的设备">
        <Text type="secondary" style={{ fontSize: 11 }}>
          勾选后该设备在面板、任务栏、悬浮框里都不再出现；随时取消勾选即可恢复。
        </Text>
        <DevicePicker
          devices={devices}
          selected={settings.hidden}
          onChange={(keys) => onChange({ hidden: keys })}
          invert
        />
      </Section>

      <Section title="主题">
        <Segmented
          block
          value={settings.theme}
          options={[
            { label: "跟随系统", value: "auto" },
            { label: "深色", value: "dark" },
            { label: "浅色", value: "light" },
          ]}
          onChange={(v) => onChange({ theme: String(v) })}
        />
      </Section>
    </div>
  );
}
