import { useState } from "react";
import { MIN_PASSWORD_CHARS, passwordStrength } from "./vaultView";

// Two characters or more: single-character keys would leak into the phrase-by-phrase translator.
const STRENGTH_TEXT = ["", "较弱", "一般", "较强", "很强"];

export function PasswordStrength({ password }: { password: string }) {
  const score = passwordStrength(password);
  return (
    <div>
      <div className="vault-meter" aria-hidden="true">{[1, 2, 3, 4].map((level) => <span key={level} className={score >= level ? "on" : ""} />)}</div>
      {score > 0 && <div className="vault-sub" style={{ margin: "4px 0 0" }}>强度：{STRENGTH_TEXT[score]}</div>}
    </div>
  );
}

export function PasswordForm({ submitLabel, busy, onSubmit }: { submitLabel: string; busy: boolean; onSubmit: (password: string) => void }) {
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [problem, setProblem] = useState("");

  function submit() {
    if ([...password].length < MIN_PASSWORD_CHARS) { setProblem("主密码至少需要 12 个字符。"); return; }
    if (password !== confirm) { setProblem("两次输入的主密码不一致。"); return; }
    setProblem("");
    onSubmit(password);
  }

  return (
    <form className="vault-form" onSubmit={(event) => { event.preventDefault(); submit(); }}>
      <label>主密码（至少 12 个字符）<input className="ip full" type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} /></label>
      <PasswordStrength password={password} />
      <label>确认主密码<input className="ip full" type="password" autoComplete="new-password" value={confirm} onChange={(e) => setConfirm(e.target.value)} /></label>
      {problem && <div className="vault-warn">{problem}</div>}
      <div className="vault-actions"><button className="pr sm" type="submit" disabled={busy}>{submitLabel}</button></div>
    </form>
  );
}
