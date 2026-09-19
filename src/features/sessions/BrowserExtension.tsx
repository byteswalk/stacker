import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ConfirmModal, useBusyRead } from "../../ui";
import { webchatConnect, webchatDisconnect, webchatOpen, webchatStatus } from "./api";
import { errorMessage, type WebBrowser, type WebchatStatus, type WebHostState } from "./types";

const BROWSER_LABEL: Record<WebBrowser, string> = { chrome: "Chrome", edge: "Edge" };
const REGISTRY_KEY: Record<WebBrowser, string> = {
  chrome: "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.stacker.webchat",
  edge: "HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.stacker.webchat",
};
const STATE_LABEL: Record<WebHostState, string> = { off: "未连接", connected: "已连接", stale: "登记指向其他位置，请重新连接" };

/** Settings block: where the extension folder is, how to load it, and the Chrome / Edge registration. */
export function BrowserExtension() {
  const { tr: t, locale } = useI18n();
  const read = useBusyRead();
  const [status, setStatus] = useState<WebchatStatus | null>(null);
  const [asking, setAsking] = useState<WebBrowser | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(() => {
    read("正在读取浏览器插件状态", webchatStatus)
      .then((next) => { setStatus(next); setError(""); })
      .catch((e) => setError(errorMessage(e)));
  }, [read]);
  useEffect(() => { load(); }, [load]);

  async function change(browser: WebBrowser, connect: boolean) {
    setWorking(true); setError("");
    try {
      setStatus(await (connect ? webchatConnect(browser) : webchatDisconnect(browser)));
      setAsking(null);
    } catch (e) { setError(errorMessage(e)); }
    finally { setWorking(false); }
  }

  const time = (ms: number | null) => (ms ? new Date(ms).toLocaleString(locale) : t("从未"));
  if (!status) return error ? <div className="session-source"><p role="alert" className="session-error">{t(error)}</p></div> : null;
  const c = status.counts;
  return <div className="session-source browser-extension">
    <div className="session-source-head">
      <b>{t("浏览器插件")}</b>
      <small>{t("「Stacker 网页对话」插件管理 ChatGPT、Claude 等网站上的对话。连接后，插件的对话列表、已读取的正文、文件夹、标签、备注和摘录会同步到 Stacker，可在「网页对话」标签查看、搜索和生成摘要；插件的导出也会直接存到 Stacker 的导出目录。")}</small>
    </div>
    <div className="session-source-edit">
      <code title={status.extensionDir}>{status.extensionDir}</code>
      <button className="gh sm" disabled={!status.extensionFound} onClick={() => void webchatOpen("extension").catch((e) => setError(errorMessage(e)))}><i className="ti ti-folder-open" />{t("打开文件夹")}</button>
    </div>
    {!status.extensionFound && <small className="warn">{t("没有找到插件文件夹。开发时请先运行 npm run ext:build。")}</small>}
    <ol className="session-note extension-steps">
      <li>{t("在 Chrome 打开 chrome://extensions，或在 Edge 打开 edge://extensions，打开「开发者模式」。")}</li>
      <li>{t("点「加载已解压的扩展程序」，选择上面的插件文件夹。")}</li>
      <li>{t("在下方点对应浏览器的「连接」，然后在扩展管理页点插件的「重新加载」。")}</li>
    </ol>
    {status.browsers.map((b) => <div className="data-location" key={b.browser}>
      <div className="data-location-text">
        <b>{BROWSER_LABEL[b.browser]}</b>
        <small className={b.state === "stale" ? "warn" : ""}>{t(STATE_LABEL[b.state])}</small>
      </div>
      <div className="session-actions">
        {b.state !== "connected" && <button className="pr sm" disabled={working} onClick={() => setAsking(b.browser)}>{t("连接")}</button>}
        {b.state !== "off" && <button className="gh sm" disabled={working} onClick={() => void change(b.browser, false)}>{t("断开")}</button>}
      </div>
    </div>)}
    <small className="session-note">{t("最近连接")}：{time(status.lastHelloAt)} · {t("最近同步")}：{time(status.lastSyncAt)}</small>
    <small className="session-note">{t("账号")} {c.accounts} · {t("对话")} {c.conversations} · {t("已存正文")} {c.bodies} · {t("文件夹")} {c.folders} · {t("摘录")} {c.excerpts}</small>
    {error && <p role="alert" className="session-error">{t(error)}</p>}
    {asking && <ConfirmModal title={`${t("连接")} ${BROWSER_LABEL[asking]}`} icon="ti-plug-connected" busy={working}
      message={`${t("将在当前用户的注册表写入")} ${REGISTRY_KEY[asking]}${t("，并在 Stacker 数据目录保存一个登记文件。浏览器插件之后可以启动 Stacker 同步数据，不会打开窗口。点「断开」会删除这两处。")}`}
      confirmLabel={t("连接")} onConfirm={() => void change(asking, true)} onClose={() => setAsking(null)} />}
  </div>;
}
