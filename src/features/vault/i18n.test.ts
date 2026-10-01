import { describe, expect, it } from "vitest";
import { translateText } from "../../i18n";
import { lockReasonText } from "./labels";

const CHINESE = /[㐀-鿿]/;
const en = (text: string) => translateText(text, "en-US");

describe("vault English copy", () => {
  it("translates the recovery key description completely", () => {
    const text = en("忘记主密码时，可使用恢复密钥解锁保管库。恢复密钥仅显示一次；主密码与恢复密钥均丢失时，数据无法恢复。");
    expect(text).toBe("If you forget the master password, the recovery key unlocks the vault. It is shown only once; if both are lost, the data cannot be recovered.");
  });

  it("translates the sidebar name and the error messages", () => {
    expect(en("密钥保管")).toBe("Key Vault");
    expect(en("主密码不正确。")).toBe("Incorrect master password.");
    expect(en("私钥超过 16 KB，请确认粘贴的内容。")).toBe("The private key is over 16 KB. Check what you pasted.");
  });

  it("translates the lock reasons, including the idle minutes", () => {
    expect(en(lockReasonText("idle", 5))).toBe("Idle for 5 minutes, so the vault is locked.");
    expect(en(lockReasonText("session", 5))).toBe("Windows was locked, so the vault is locked.");
    expect(en(lockReasonText("sleep", 5))).toBe("The computer went to sleep, so the vault is locked.");
  });

  it("translates the texts built from template pieces around a number", () => {
    expect(en("已导入 3 项。原文件未做任何改动。")).toBe("Imported 3 item(s). The original files were not changed.");
    expect(en("已导入：新增 2，更新 1.")).toBe("Imported: added 2, updated 1.");
    expect(en("导入所选")).toBe("Import selected");
    expect(en("删除「GitHub」？可在回收站保留 30 天。")).toBe("Delete \"GitHub\"? It stays in the trash for 30 days.");
    expect(en("RSA · 4096 位")).toBe("RSA · 4096-bit");
  });

  it("leaves no Chinese in the Rust data field names and the platform presets", () => {
    for (const text of ["私钥", "公钥", "账号", "火山方舟", "额度/周期", "已在保管库（旧值）"]) {
      expect(CHINESE.test(en(text)), text).toBe(false);
    }
  });

  it("uses vault-specific wording for buttons that collide with shared keys", () => {
    const expected: Record<string, string> = {
      "删除条目": "Delete entry",
      "确认修改": "Change password",
      "显示内容": "Show value",
      "隐藏内容": "Hide value",
      "确认导入": "Import",
      "立即清除": "Delete now",
      "已到期": "Expired",
      "已保存。": "Saved.",
      "复制": "Copy",
      "取消": "Cancel",
      "编辑": "Edit",
      "预览": "Preview",
      "历史": "History",
      "恢复": "Restore",
    };
    for (const [chinese, english] of Object.entries(expected)) expect(en(chinese), chinese).toBe(english);
  });

  it("does not touch full-width punctuation outside the vault texts", () => {
    expect(en("WorkBuddy 桌面端（中国版）")).toBe("WorkBuddy desktop（China Edition）");
    expect(en("默认 Python（Stacker 设置）")).toBe("Default Python（Stacker Settings）");
    expect(en("（备注）")).toBe("（Notes）");
    expect(en("这是一段没有翻译的话。")).toBe("这是一段没有翻译的话。");
  });
});
