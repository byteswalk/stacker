import { createRoot } from "react-dom/client";
import { requestFlush } from "../../lib/bridgeMessages";
import { onOutboxChange } from "../../lib/db";
import "../style.css";
import { App } from "./App";

// Local changes made on this page reach Stacker through the background.
onOutboxChange(() => requestFlush());

createRoot(document.getElementById("root")!).render(<App />);
