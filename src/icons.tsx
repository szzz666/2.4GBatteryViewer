import {
  AimOutlined,
  CustomerServiceOutlined,
  DesktopOutlined,
  LaptopOutlined,
  QuestionCircleOutlined,
  UsbOutlined,
} from "@ant-design/icons";
import type { CSSProperties, ReactNode } from "react";

/**
 * 设备类型图标 —— 全部来自 @ant-design/icons，不用 emoji / 自绘 SVG。
 * 鼠标：Aim（指向/定位）；键盘：笔记本；一体机：桌面；耳机：客服耳机；接收器：USB。
 */
export function deviceIcon(kind: string, style?: CSSProperties): ReactNode {
  const props = { style };
  switch (kind) {
    case "headset":
      return <CustomerServiceOutlined {...props} />;
    case "mouse":
      return <AimOutlined {...props} />;
    case "keyboard":
      return <LaptopOutlined {...props} />;
    case "combo":
      return <DesktopOutlined {...props} />;
    case "receiver":
      return <UsbOutlined {...props} />;
    default:
      return <QuestionCircleOutlined {...props} />;
  }
}