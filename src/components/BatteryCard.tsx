import { Card, Progress, Space, Tag, Tooltip, Typography } from "antd";
import { ThunderboltFilled } from "@ant-design/icons";
import type { DeviceSnapshot } from "../types";
import { levelColor } from "../theme";
import { deviceIcon } from "../icons";

const { Text } = Typography;

const STATE_LABEL: Record<string, string> = {
  charging: "充电中",
  full: "已充满",
  discharging: "放电中",
  unknown: "状态未知",
};

function timeAgo(ts: number): string {
  if (!ts) return "尚未读取";
  const diff = Math.max(0, Date.now() - ts);
  if (diff < 2000) return "刚刚";
  if (diff < 60_000) return `${Math.round(diff / 1000)} 秒前`;
  if (diff < 3_600_000) return `${Math.round(diff / 60_000)} 分钟前`;
  return `${Math.round(diff / 3_600_000)} 小时前`;
}

interface Props {
  snap: DeviceSnapshot;
  onOpenDebug?: () => void;
}

export function BatteryCard({ snap, onOpenDebug }: Props) {
  const { device, reading } = snap;
  const percent = reading?.percent ?? null;
  const state = reading?.state ?? "unknown";
  const color = levelColor(percent);

  const tags = [];
  if (state === "charging") {
    tags.push(
      <Tag key="c" color="processing" icon={<ThunderboltFilled />}>
        充电中
      </Tag>,
    );
  }
  if (state === "full") tags.push(<Tag key="f" color="success">已充满</Tag>);

  return (
    <Card
      size="small"
      style={{ marginBottom: 12, borderColor: percent == null ? undefined : color + "40" }}
      styles={{ body: { padding: 14 } }}
    >
      <Space align="start" style={{ width: "100%", justifyContent: "space-between" }}>
        <Space align="start" size={10}>
          <span
            style={{
              width: 34,
              height: 34,
              borderRadius: 9,
              display: "grid",
              placeItems: "center",
              fontSize: 16,
              background: color + "22",
              color,
            }}
          >
            {deviceIcon(device.kind)}
          </span>
          <div>
            <Text strong style={{ fontSize: 14 }}>
              {device.name}
            </Text>
            <div>
              <Text type="secondary" style={{ fontSize: 11 }}>
                {device.manufacturer ? `${device.manufacturer} · ` : ""}
                {device.vendorId.toString(16).toUpperCase().padStart(4, "0")}:
                {device.productId.toString(16).toUpperCase().padStart(4, "0")}
              </Text>
            </div>
          </div>
        </Space>
        <Space size={4}>{tags}</Space>
      </Space>

      {percent === null ? (
        <div style={{ marginTop: 12 }}>
          <Space size={6} align="center">
            <Tag color="warning" style={{ marginInlineEnd: 0 }}>
              未读取
            </Tag>
            <Text style={{ fontSize: 13, fontWeight: 550 }}>未能读取电量</Text>
          </Space>
          <div style={{ marginTop: 4 }}>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {reading?.note ?? "该设备未提供协议，或处于休眠状态"}
            </Text>
          </div>
          {onOpenDebug && (
            <Typography.Link style={{ fontSize: 12, marginTop: 6, display: "inline-block" }} onClick={onOpenDebug}>
              打开调试台手动标定 →
            </Typography.Link>
          )}
        </div>
      ) : (
        <div style={{ marginTop: 10 }}>
          <Space align="baseline" size={4}>
            <span style={{ fontSize: 40, fontWeight: 650, lineHeight: 1, color }}>{percent}</span>
            <span style={{ fontSize: 15, color }}>%</span>
            <Text type="secondary" style={{ fontSize: 12, marginLeft: 6 }}>
              {STATE_LABEL[state]}
            </Text>
          </Space>
          <Progress
            percent={percent}
            showInfo={false}
            strokeColor={color}
            trailColor="rgba(128,128,128,0.18)"
            size={["100%", 8]}
            style={{ margin: "8px 0 6px" }}
          />
          <Space style={{ width: "100%", justifyContent: "space-between" }} size={8}>
            <Tooltip title={reading?.source}>
              <Text type="secondary" style={{ fontSize: 11 }} ellipsis>
                数据来源：{reading?.source}
              </Text>
            </Tooltip>
            <Text type="secondary" style={{ fontSize: 11, whiteSpace: "nowrap" }}>
              {reading?.voltageMv ? `${(reading.voltageMv / 1000).toFixed(2)}V · ` : ""}
              {timeAgo(reading?.updatedAt ?? 0)}
            </Text>
          </Space>
        </div>
      )}
    </Card>
  );
}