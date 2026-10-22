import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

// 标记窗口类型，供视图切换使用（WebView 的 label 通过 URL 查询参数传入）
const params = new URLSearchParams(window.location.search);
const label = params.get("label");
if (label) document.documentElement.dataset.windowLabel = label;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
