import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { StatusBar } from "./features/bar/StatusBar";
import "./index.css";

// The compact status bar window loads the same bundle with ?view=bar.
const isBar = new URLSearchParams(window.location.search).get("view") === "bar";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isBar ? <StatusBar /> : <App />}</React.StrictMode>,
);
