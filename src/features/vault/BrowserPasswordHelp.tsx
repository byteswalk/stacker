import { useState } from "react";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";

type Browser = "chrome" | "edge" | "firefox";

/** Where each browser keeps its passwords, and how they go out of it and back in. */
const GUIDE: Record<Browser, { name: string; page: string; exportSteps: string[]; importSteps: string[]; note?: string }> = {
  chrome: {
    name: "Chrome",
    page: "chrome://password-manager/settings",
    exportSteps: [
      "在地址栏打开 chrome://password-manager/settings",
      "找到「导出密码」，点旁边的「下载文件」",
      "按提示验证 Windows 登录密码或 PIN，Chrome 会保存一个 CSV 文件",
    ],
    importSteps: [
      "在地址栏打开 chrome://password-manager/settings",
      "找到「导入密码」，点旁边的「选择文件」",
      "选中 Stacker 导出的 Chrome 格式 CSV 文件，已有的同网站同账号会让你选择保留哪个",
    ],
  },
  edge: {
    name: "Edge",
    page: "edge://wallet/passwords",
    exportSteps: [
      "在地址栏打开 edge://wallet/passwords",
      "点密码列表右上角的「…」，选「导出密码」",
      "按提示验证 Windows 登录密码或 PIN，Edge 会保存一个 CSV 文件",
    ],
    importSteps: [
      "在地址栏打开 edge://wallet/passwords",
      "点密码列表右上角的「…」，选「导入密码」",
      "来源选「密码 CSV 文件」，再选中 Stacker 导出的 Edge 格式 CSV 文件",
    ],
    note: "旧版 Edge 的入口在「设置 → 个人资料 → 密码」。",
  },
  firefox: {
    name: "Firefox",
    page: "about:logins",
    exportSteps: [
      "在地址栏打开 about:logins",
      "点右上角的「…」，选「导出密码…」",
      "确认导出并验证 Windows 登录密码，Firefox 会保存一个 CSV 文件",
    ],
    importSteps: [
      "在地址栏打开 about:logins",
      "点右上角的「…」，选「从文件导入…」",
      "选中 Stacker 导出的 Firefox 格式 CSV 文件",
    ],
    note: "Firefox 只按网站的协议、域名和端口保存登录，不记路径；同一网站同一账号的几条导入后会合成一条。",
  },
};

/** How to bring browser passwords into Stacker, and take them back out to a browser. */
export function BrowserPasswordHelp({ onClose, initial = "chrome" }: { onClose: () => void; initial?: Browser }) {
  const { tr } = useI18n();
  const [browser, setBrowser] = useState<Browser>(initial);
  const guide = GUIDE[browser];
  return <Modal wide title="浏览器密码导入导出说明" icon="ti-help-circle" onClose={onClose}
    footer={<button className="pr sm" onClick={onClose}>知道了</button>}>
    <div className="pw-help">
      <div className="seg" role="tablist" aria-label={tr("浏览器")}>
        {(Object.keys(GUIDE) as Browser[]).map((id) => <button key={id} role="tab" aria-selected={browser === id}
          className={browser === id ? "on" : ""} onClick={() => setBrowser(id)}>{GUIDE[id].name}</button>)}
      </div>
      <section>
        <h4><i className="ti ti-world-download" /> {tr("从 {name} 导入到 Stacker").replace("{name}", guide.name)}</h4>
        <ol>
          {guide.exportSteps.map((step) => <li key={step}>{tr(step)}</li>)}
          <li>{tr("在 Stacker 的密钥保管点右上角「…」→「导入浏览器密码」，选中这个 CSV 文件")}</li>
        </ol>
      </section>
      <section>
        <h4><i className="ti ti-world-upload" /> {tr("从 Stacker 导入到 {name}").replace("{name}", guide.name)}</h4>
        <ol>
          <li>{tr("在 Stacker 的密钥保管点右上角「…」→「导出为浏览器密码」导出全部；或在列表里勾选条目，点底部的「导出给浏览器」只导出选中的")}</li>
          <li>{tr("选 {name}，输入主密码，选择保存位置").replace("{name}", guide.name)}</li>
          {guide.importSteps.map((step) => <li key={step}>{tr(step)}</li>)}
        </ol>
      </section>
      {guide.note && <div className="pw-help-note"><i className="ti ti-info-circle" /> {tr(guide.note)}</div>}
      <div className="pw-help-note warn"><i className="ti ti-alert-triangle" /> {tr("CSV 文件里的密码是明文。导入完成后请删除它，并清空回收站；不要通过聊天工具或网盘传给别人。")}</div>
      <div className="pw-help-note"><i className="ti ti-puzzle" /> {tr("只是想在浏览器里用这些密码，也可以不导出：装上 Stacker 的浏览器插件（Chrome、Edge），把条目放进 Windows 凭据，登录页就能直接填。")}</div>
    </div>
  </Modal>;
}

/** A small "?" that opens the guide, for a dialog's corner. */
export function BrowserPasswordHelpButton() {
  const { tr } = useI18n();
  const [open, setOpen] = useState(false);
  return <>
    <button type="button" className="gh xs" title={tr("浏览器密码导入导出说明")} aria-label={tr("浏览器密码导入导出说明")} onClick={() => setOpen(true)}>
      <i className="ti ti-help-circle" /> {tr("说明")}
    </button>
    {open && <BrowserPasswordHelp onClose={() => setOpen(false)} />}
  </>;
}
