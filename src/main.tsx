import ReactDOM from "react-dom/client";
import App from "./App";

// Tauri app: don't show the browser context menu (Inspect, Reload, etc).
document.addEventListener(
  "contextmenu",
  (event) => {
    event.preventDefault();
  },
  { capture: true }
);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
