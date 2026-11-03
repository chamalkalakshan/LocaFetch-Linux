import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// Linux windows are opaque (see src-tauri/tauri.linux.conf.json): WebKitGTK
// often fails to paint transparent windows, so there are no see-through corners.
if (navigator.userAgent.includes("Linux")) {
  document.documentElement.classList.add("opaque-window");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
