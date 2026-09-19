import { createRoot } from "react-dom/client";
import { t } from "../../i18n";

createRoot(document.getElementById("root")!).render(<h1>{t("Stacker 网页对话")}</h1>);
