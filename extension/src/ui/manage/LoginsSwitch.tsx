import { useEffect, useState } from "react";
import { Switch, Typography } from "antd";
import { t } from "../../i18n";
import { LOGIN_MATCHES } from "../../logins/background";

/** The switch for website passwords: asks the browser for every site, or gives it back. */
export function LoginsSwitch() {
  const [on, setOn] = useState(false);
  const [busy, setBusy] = useState(false);
  const available = typeof chrome !== "undefined" && !!chrome.permissions;

  useEffect(() => {
    if (!available) return;
    void chrome.permissions.contains({ origins: LOGIN_MATCHES }).then(setOn).catch(() => setOn(false));
  }, [available]);

  // The request has to come straight from the click: the browser shows its own prompt.
  async function change(next: boolean) {
    if (!available) return;
    setBusy(true);
    try {
      const done = next ? await chrome.permissions.request({ origins: LOGIN_MATCHES }) : await chrome.permissions.remove({ origins: LOGIN_MATCHES });
      if (done) setOn(next);
    } finally {
      setBusy(false);
    }
  }

  return <div>
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
      <Typography.Text type="secondary">{t("网站密码")}</Typography.Text>
      <Switch size="small" checked={on} loading={busy} disabled={!available} onChange={(next) => void change(next)} />
    </div>
    <Typography.Text type="secondary" style={{ fontSize: 11 }}>
      {on ? t("已开启：只在你点按钮时保存或填充，从不自动填入。") : t("登录网站时提示保存到 Stacker 密钥保管，并在登录页提供填充。需要授权插件访问所有网站。")}
    </Typography.Text>
  </div>;
}
