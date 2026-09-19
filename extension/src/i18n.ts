/** English for every Chinese UI string; Chinese is the source text. */
export const EN: Record<string, string> = {
  "Stacker 网页对话": "Stacker Web Chats",
};

export function t(text: string): string {
  const english = typeof navigator !== "undefined" && !navigator.language.toLowerCase().startsWith("zh");
  return english ? EN[text] ?? text : text;
}
