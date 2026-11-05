import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./style.css";

const root = document.getElementById("root");
if (!root) throw new Error("Missing application root");
if (
  import.meta.env.PROD &&
  window.isSecureContext &&
  "serviceWorker" in navigator
) {
  navigator.serviceWorker.register("/sw.js", { scope: "/" }).catch(() => {});
}
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
