import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles/tokens.css";
import "./styles/app.css";
import { applyTheme, loadTheme } from "./lib/theme";
import { applyLanguage, loadLanguage } from "./lib/i18n";

// Before the first paint, so a dark window doesn't flash light.
applyTheme(loadTheme());
applyLanguage(loadLanguage());

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
