/** The bits of chrome.runtime.MessageSender the trust check needs, kept minimal so it is easy to test. */
export interface Sender { id?: string; url?: string }

/**
 * True only for the extension's own pages (manage, popup): matching `sender.id` alone is not enough,
 * since a content script injected into a website shares the extension's id but its `sender.url` is
 * the site's own URL, not the extension's.
 */
export function isTrustedSender(sender: Sender, extensionId: string, extensionUrl: string): boolean {
  return sender.id === extensionId && typeof sender.url === "string" && sender.url.startsWith(extensionUrl);
}
