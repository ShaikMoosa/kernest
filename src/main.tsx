import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { PreviewWindow } from "./PreviewWindow";
import { isPreviewWindow } from "./lib/previewEvents";
import "./styles/tokens.css";
import "./styles/app.css";
import { applyTheme, loadTheme } from "./lib/theme";
import { applyLanguage, loadLanguage } from "./lib/i18n";

// Before the first paint, so a dark window doesn't flash light.
applyTheme(loadTheme());
applyLanguage(loadLanguage());

// The same page serves the separate preview window (SMO-36), opened with ?window=preview.
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {isPreviewWindow(window.location.search) ? <PreviewWindow /> : <App />}
  </StrictMode>,
);
