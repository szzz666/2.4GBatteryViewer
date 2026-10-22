import { Checkbox, Empty, Flex, Typography } from "antd";
import type { DeviceSnapshot } from "../types";

const { Text } = Typography;

interface Props {
  devices: DeviceSnapshot[];
  selected: string[];
  onChange: (keys: string[]) => void;
  emptyHint?: string;
  /** 反选模式：勾选表示"排除"（用于隐藏设备） */
  invert?: boolean;
}

/** 多选“显示哪些设备”，空选 = 自动显示所有能读到电量的设备 */
export function DevicePicker({ devices, selected, onChange, emptyHint, invert }: Props) {
  const toggle = (key: string, on: boolean) => {
    onChange(on ? [...selected, key] : selected.filter((k) => k !== key));
  };

  if (devices.length === 0) {
    return <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="暂未检测到设备" />;
  }

  return (
    <Flex vertical gap={6} style={{ marginTop: 8 }}>
      {devices.map((d) => {
        const on = selected.includes(d.device.key);
        const pct = d.reading?.percent;
        return (
          <Flex
            key={d.device.key}
            align="center"
            justify="space-between"
            style={{
              padding: "6px 10px",
              borderRadius: 8,
              border: `1px solid ${on ? "var(--ant-color-primary)" : "var(--ant-color-border-secondary)"}`,
              background: on ? "var(--ant-color-primary-bg)" : "transparent",
            }}
          >
            <Checkbox
              checked={on}
              onChange={(e) => toggle(d.device.key, e.target.checked)}
              style={{ fontSize: 12 }}
            >
              <Text style={{ fontSize: 12 }} ellipsis={{ tooltip: d.device.name }}>
                {d.device.name}
              </Text>
            </Checkbox>
            <Text type="secondary" style={{ fontSize: 11, marginLeft: 8, whiteSpace: "nowrap" }}>
              {pct == null ? "—" : `${pct}%`}
            </Text>
          </Flex>
        );
      })}
      <Flex justify="space-between" align="center">
        <Text type="secondary" style={{ fontSize: 11 }}>
          {selected.length === 0
            ? emptyHint ?? (invert ? "全部显示" : "未勾选 = 自动显示所有读得到电量的设备")
            : invert
              ? `已隐藏 ${selected.length} 台`
              : `已选 ${selected.length} 台`}
        </Text>
        {selected.length > 0 && (
          <Typography.Link style={{ fontSize: 11 }} onClick={() => onChange([])}>
            {invert ? "全部显示" : "恢复自动"}
          </Typography.Link>
        )}
      </Flex>
    </Flex>
  );
}
