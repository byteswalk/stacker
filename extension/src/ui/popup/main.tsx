import { createRoot } from "react-dom/client";
import { requestFlush } from "../../lib/bridgeMessages";
import { onOutboxChange } from "../../lib/db";
import "../style.css";
import { Popup } from "./Popup";

// Local changes made on this page reach Stacker through the background.
onOutboxChange(() => requestFlush());

createRoot(document.getElementById("root")!).render(<Popup />);
