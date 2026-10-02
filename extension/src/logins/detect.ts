/** Finding a page's login fields: what was typed into them, and where to fill. */

const USER_TYPES = new Set(["text", "email", "tel", ""]);

function visible(input: HTMLInputElement): boolean {
  if (input.disabled || input.type === "hidden") return false;
  const rect = input.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0;
}

/** The password fields of a form (or of the page, when the field has no form). */
export function passwordFields(scope: ParentNode): HTMLInputElement[] {
  return [...scope.querySelectorAll<HTMLInputElement>("input[type=password]")].filter(visible);
}

/**
 * The account field that goes with a password field: one marked as a username or e-mail,
 * else the last visible text field before it in the same form.
 */
export function userFieldFor(password: HTMLInputElement): HTMLInputElement | null {
  const scope: ParentNode = password.form ?? password.ownerDocument;
  const inputs = [...scope.querySelectorAll<HTMLInputElement>("input")].filter((input) => USER_TYPES.has(input.type) && visible(input));
  const before = inputs.filter((input) => input.compareDocumentPosition(password) & Node.DOCUMENT_POSITION_FOLLOWING);
  const marked = before.find((input) => /username|email/i.test(input.autocomplete) || /user|login|email|account|mail|phone|账号|用户/i.test(`${input.name} ${input.id}`));
  return marked ?? before.at(-1) ?? null;
}

/**
 * What a submitted form holds: the account and the password to keep. With several password
 * fields (changing a password) the new one is kept: one marked `new-password`, else the last.
 */
export function captureFrom(scope: ParentNode): { user: string; password: string } | null {
  const filled = passwordFields(scope).filter((input) => input.value.length > 0);
  if (!filled.length) return null;
  const password = filled.find((input) => input.autocomplete === "new-password") ?? filled.at(-1)!;
  const user = userFieldFor(filled[0])?.value.trim() ?? "";
  return { user, password: password.value };
}

/** Sets a field the way typing would, so pages built on React and the like notice it. */
export function fillField(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  if (setter) setter.call(input, value);
  else input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}
