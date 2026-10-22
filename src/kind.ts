/** 设备类型 → emoji 图标（antd 没有鼠标/键盘图标，emoji 在小尺寸下更好认） */
export function kindEmoji(kind: string): string {
  switch (kind) {
    case "mouse":
      return "\u{1F5B1}\u{FE0F}";
    case "keyboard":
      return "\u{2328}\u{FE0F}";
    case "combo":
      return "\u{1F5B1}\u{FE0F}";
    case "headset":
      return "\u{1F3A7}";
    case "receiver":
      return "\u{1F4E1}";
    default:
      return "\u{1F50C}";
  }
}
