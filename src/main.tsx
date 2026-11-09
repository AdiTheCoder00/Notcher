import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "@fontsource-variable/inter";
import "./styles.css";

const view = new URLSearchParams(window.location.search).get("window") ?? "main";
document.documentElement.dataset.window = view;

const container = document.getElementById("root");
if (container) {
  createRoot(container).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
