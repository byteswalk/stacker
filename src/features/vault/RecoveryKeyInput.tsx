import { useRef } from "react";
import { useToast } from "../../ui";
import { vaultApi, vaultError } from "./api";

export const KEY_GROUPS = 8;
const GROUP_CHARS = 4;

export const emptyKey = (): string[] => Array.from({ length: KEY_GROUPS }, () => "");
export const keyComplete = (groups: string[]) => groups.every((group) => group.length === GROUP_CHARS);
export const keyText = (groups: string[]) => groups.join("-");

/** Letters and digits only, upper case: dashes, spaces and line breaks of a copied key drop out. */
function clean(text: string): string {
  return text.toUpperCase().replace(/[^0-9A-Z]/g, "");
}

/** `text` laid into the groups from `start` on; what is before `start` stays, what is after is replaced. */
export function fillKey(groups: string[], start: number, text: string): string[] {
  const chars = clean(text);
  const next = groups.slice(0, start);
  for (let index = start; index < KEY_GROUPS; index += 1) {
    next.push(chars.slice((index - start) * GROUP_CHARS, (index - start + 1) * GROUP_CHARS));
  }
  return next;
}

/** The recovery key as its eight groups of four, typed or pasted whole into any of them. */
export function RecoveryKeyInput({ groups, onChange, disabled, autoFocus }: {
  groups: string[]; onChange: (groups: string[]) => void; disabled?: boolean; autoFocus?: boolean;
}) {
  const toast = useToast();
  const boxes = useRef<(HTMLInputElement | null)[]>([]);
  const focus = (index: number) => {
    const box = boxes.current[Math.max(0, Math.min(KEY_GROUPS - 1, index))];
    box?.focus();
    box?.select();
  };

  // A whole key always lands from the first group, wherever the cursor was.
  function spread(index: number, text: string) {
    const chars = clean(text);
    const start = chars.length >= KEY_GROUPS * GROUP_CHARS ? 0 : index;
    const next = fillKey(groups, start, chars);
    onChange(next);
    const firstOpen = next.findIndex((group) => group.length < GROUP_CHARS);
    focus(firstOpen === -1 ? KEY_GROUPS - 1 : firstOpen);
  }

  function type(index: number, text: string) {
    const chars = clean(text);
    if (chars.length > GROUP_CHARS) { spread(index, chars); return; }
    onChange(groups.map((group, at) => (at === index ? chars : group)));
    if (chars.length === GROUP_CHARS && index < KEY_GROUPS - 1) focus(index + 1);
  }

  function key(index: number, event: React.KeyboardEvent<HTMLInputElement>) {
    const box = event.currentTarget;
    if (event.key === "Backspace" && box.value === "" && index > 0) {
      event.preventDefault();
      onChange(groups.map((group, at) => (at === index - 1 ? group.slice(0, -1) : group)));
      boxes.current[index - 1]?.focus();
    } else if (event.key === "ArrowLeft" && box.selectionStart === 0 && box.selectionEnd === 0 && index > 0) {
      event.preventDefault();
      boxes.current[index - 1]?.focus();
    } else if (event.key === "ArrowRight" && box.selectionStart === box.value.length && index < KEY_GROUPS - 1) {
      event.preventDefault();
      boxes.current[index + 1]?.focus();
    }
  }

  // The button reads the clipboard in the backend: the page itself is never given clipboard access.
  async function paste() {
    try {
      const text = clean(await vaultApi.clipboardText() ?? "");
      if (!text) { toast("剪贴板里没有可用的恢复密钥。", "info"); return; }
      spread(0, text);
    } catch (error) { toast(vaultError(error), "err"); }
  }

  return (
    <div className="vault-keyin">
      <div className="vault-keyin-boxes" translate="no">
        {groups.map((group, index) => (
          <span className="vault-keyin-group" key={index}>
            <input ref={(element) => { boxes.current[index] = element; }} className="ip" value={group} disabled={disabled}
              autoFocus={autoFocus && index === 0} autoComplete="off" spellCheck={false} maxLength={KEY_GROUPS * GROUP_CHARS + KEY_GROUPS}
              aria-label={`${index + 1} / ${KEY_GROUPS}`}
              onChange={(event) => type(index, event.target.value)}
              onKeyDown={(event) => key(index, event)}
              onFocus={(event) => event.target.select()}
              onPaste={(event) => { event.preventDefault(); spread(index, event.clipboardData.getData("text")); }} />
            {index < KEY_GROUPS - 1 && <i aria-hidden="true">-</i>}
          </span>
        ))}
      </div>
      <div className="vault-keyin-tools">
        <span className="vault-sub">共 8 组、每组 4 位；可直接 Ctrl+V 粘贴整串。</span>
        <button type="button" className="gh sm" disabled={disabled} onClick={() => void paste()}><i className="ti ti-clipboard" /> 粘贴</button>
        <button type="button" className="gh sm" disabled={disabled || groups.every((group) => !group)}
          onClick={() => { onChange(emptyKey()); focus(0); }}><i className="ti ti-eraser" /> 清空</button>
      </div>
    </div>
  );
}
