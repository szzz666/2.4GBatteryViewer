import type { ThemeConfig } from "antd";
import { theme as antdTheme } from "antd";

/** 深色 / 浅色两套 antd 主题，跟随应用设置 */
export function buildTheme(dark: boolean): ThemeConfig {
  return {
    // 开启 CSS 变量，组件里才能用 var(--ant-color-*)
    cssVar: { key: dark ? "app-dark" : "app-light" },
    hashed: false,
    algorithm: dark ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
    token: {
      colorPrimary: "#4f9cf9",
      colorSuccess: "#34d399",
      colorWarning: "#fbbf24",
      colorError: "#f87171",
      borderRadius: 10,
      fontSize: 13,
      fontFamily:
        '"Segoe UI", "Microsoft YaHei UI", system-ui, -apple-system, sans-serif',
    },
    components: {
      Card: {
        paddingLG: 14,
      },
      Progress: {
        defaultColor: "#4f9cf9",
      },
    },
  };
}

/** 把 theme 设置（auto/dark/light）解析成最终的深浅色 */
export function resolveDark(theme: string): boolean {
  if (theme === "dark") return true;
  if (theme === "light") return false;
  return (
    typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

/** 电量等级对应的 antd 语义色 */
export function levelColor(percent: number | null): string {
  if (percent == null) return "#6b7280";
  if (percent <= 15) return "#f87171";
  if (percent <= 35) return "#fbbf24";
  if (percent <= 65) return "#4f9cf9";
  return "#34d399";
}

export function levelStatus(percent: number | null): "exception" | "warning" | "normal" | "active" {
  if (percent == null) return "normal";
  if (percent <= 15) return "exception";
  if (percent <= 35) return "warning";
  return "normal";
}