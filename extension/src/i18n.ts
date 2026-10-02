/** English for every Chinese UI string; Chinese is the source text. */
export const EN: Record<string, string> = {
  "Stacker 网页对话": "Stacker Web Chats",

  // logins
  "保存到 Stacker 密钥保管？": "Save to Stacker Key Vault?",
  "保存": "Save",
  "保存并允许填充": "Save and allow filling",
  "不保存": "Not now",
  "此网站不再询问": "Never for this site",
  "已交给 Stacker，解锁保管库后收进去。": "Handed to Stacker; it goes into the vault when the vault is unlocked.",
  "没能交给 Stacker：请确认 Stacker 已连接这个浏览器。": "Could not hand it to Stacker: check that Stacker is connected to this browser.",
  "「允许填充」的登录会放进 Windows 凭据管理器，以后在这个网站点一下就能填。": "Logins allowed to fill go into Windows Credential Manager, to be filled on this site with one click.",
  "用 Stacker 填入": "Fill with Stacker",
  "网站密码": "Website passwords",
  "登录网站时提示保存到 Stacker 密钥保管，并在登录页提供填充。需要授权插件访问所有网站。": "Offers to save logins to Stacker Key Vault when you sign in, and to fill them on sign-in pages. Needs access to every site.",
  "已开启：只在你点按钮时保存或填充，从不自动填入。": "On: saves or fills only when you click; never fills by itself.",

  // manage/App.tsx
  "打开": "Open",
  "正在刷新列表": "Refreshing list",
  "共": "total",
  "新增": "added",
  "已删除": "removed",
  "正在读取正文": "Reading body",
  "正在导出": "Exporting",
  "已导出": "Exported",
  "条到下载目录的「Stacker 网页对话」文件夹": "item(s) to the “Stacker Web Chats” folder in your downloads",
  "正在打开…": "Opening…",
  "全部站点": "All sites",
  "全部账号": "All accounts",
  "账号备注名": "Account alias",
  "改备注名": "Rename",
  "刷新": "Refresh",
  "全部": "All",
  "未归入": "Unfiled",
  "重命名": "Rename",
  "文件夹名称": "Folder name",
  "删除文件夹": "Delete folder",
  "删除文件夹？其中的对话不会被删除。": "Delete this folder? Conversations in it will not be deleted.",
  "新建文件夹": "New folder",
  "移到文件夹…": "Move to folder…",
  "标签": "Tag",
  "加标签": "Add tag",
  "收藏": "Favorite",
  "导出精简版": "Export slim",
  "导出完整版": "Export full",
  "删除…": "Delete…",
  "选择一条对话查看详情": "Select a conversation to see its details",
  "接口已变化": "interface changed",
  "接口已变化，请先刷新该站点": "the site's interface changed; refresh that site first",
  "没有对话。先打开并登录 ChatGPT、Claude、Gemini、Grok 或 DeepSeek，再点「刷新」。": "No conversations. Open and sign in to ChatGPT, Claude, Gemini, Grok or DeepSeek, then click “Refresh”.",
  "未实测": "not yet verified",
  "这些站点的接口还没有在真实账号上核对过：可以刷新、读取和导出，暂不支持删除。": "These sites' interfaces haven't been checked against a real account yet: refresh, read and export work; deleting is not available yet.",

  "确定": "OK",
  "外观": "Appearance",
  "已连接 Stacker，外观两边保持一致": "Connected to Stacker; both use the same appearance",
  "跟随系统": "System",
  "深色": "Dark",
  "浅色": "Light",
  "语言": "Language",
  "跟随浏览器": "Browser",
  "导出": "Export",
  "打开网站": "Open site",
  "设置": "Settings",
  "请不要关闭此页": "Please don't close this page",
  "站点": "Site",
  "对话详情": "Conversation details",
  "更新时间": "Updated",
  "正文": "Body",
  "还没有读取正文": "The body hasn't been read yet",
  "最近同步": "Last synced",
  "从 Stacker 恢复账号备注名、文件夹、标签、收藏、备注和摘录": "Restore account aliases, folders, tags, favorites, notes and excerpts from Stacker",

  // manage/siteStatus.ts
  "未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除": "Not yet verified: this site's interface hasn't been checked against a real account, so deleting is not available yet",

  // manage/ConversationList.tsx
  "本页": "This page",
  "选中全部结果": "Select all results",
  "取消选择": "Clear selection",
  "已选": "Selected",
  "（无标题）": "(Untitled)",
  "正文已读": "Body read",
  "已归档": "Archived",

  // manage/DeleteDialog.tsx
  "删除对话": "Delete conversations",
  "将删除": "This will delete",
  "条对话。": "conversation(s).",
  "条不属于当前登录的账号，会被跳过。请先在网站上切换到对应账号。": "item(s) do not belong to the currently signed-in account and will be skipped. Switch to that account on the site first.",
  "直接删除不会留下任何副本，删除后无法恢复。确定继续吗？": "Deleting directly leaves no copy and cannot be undone. Continue?",
  "取消": "Cancel",
  "确认直接删除": "Confirm direct delete",
  "删除": "Delete",
  "条": "item(s)",
  "正在删除，请不要关闭此页…": "Deleting, please don't close this page…",
  "已完成": "Done",
  "中止": "Abort",
  "完成": "Finish",
  "精简导出后删除": "Export slim, then delete",
  "把用户和助手的正文存成 Markdown，再删除网站上的对话。": "Save the user's and assistant's messages as Markdown, then delete the conversation on the site.",
  "完整备份后删除": "Back up fully, then delete",
  "保存当前分支的完整 Markdown（含工具消息和附件名），并把同样的消息另存为 JSON，再删除网站上的对话。": "Save the current branch as full Markdown (including tool messages and attachment names) and the same messages as JSON, then delete the conversation on the site.",
  "不是当前登录的账号，会被跳过": "not the currently signed-in account; will be skipped",
  "直接删除": "Delete directly",
  "不留任何副本。": "Keeps no copy.",

  // manage/Detail.tsx
  "打开原对话": "Open original conversation",
  "未归入文件夹": "No folder",
  "移除标签": "Remove tag",
  "备注": "Note",
  "读取于": "Read at",
  "重新读取正文": "Re-read body",
  "读取正文": "Read body",
  "摘录": "Excerpts",
  "用户": "User",
  "助手": "Assistant",
  "提炼结果": "Distilled results",
  "提炼在 Stacker 里进行，这里只能查看。": "Distilling happens in Stacker; this view is read-only.",
  "已采用": "Adopted",
  "经验问答": "Experience Q&A",
  "领域要求": "Domain requirements",
  "提示词": "Reusable prompts",
  "skill 草稿": "Skill drafts",

  // manage/Filters.tsx
  "搜索标题或备注": "Search title or note",
  "搜索正文（仅已读取的对话）": "Search body (fetched conversations only)",
  "全部标签": "All tags",
  "正文：全部": "Body: all",
  "正文未读": "Body unread",
  "起始日期": "From date",
  "结束日期": "To date",
  "仅收藏": "Favorites only",
  "显示已删除": "Show deleted",

  // ui/popup/Popup.tsx
  "在 ChatGPT、Claude、Gemini、Grok 或 DeepSeek 打开一条对话后，这里会显示它。": "Open a conversation on ChatGPT, Claude, Gemini, Grok or DeepSeek to see it here.",
  "这条对话还不在列表里：请在管理页刷新该站点。": "This conversation isn't in the list yet: refresh that site on the manage page.",
  "在网页上选中文字，点出现的「存为摘录」按钮即可添加。": "Select text on the page and click the “Save as excerpt” button that appears to add one.",
  "在管理页打开": "Open in manage page",

  // content/excerpt.ts
  "存为摘录": "Save as excerpt",
  "已存": "Saved",
  "保存失败": "Save failed",

  // ui/errors.ts (ERROR_TEXT values)
  "请先在浏览器中打开并登录该网站": "Please open the site in your browser and sign in first",
  "请刷新该网站页面后重试": "Please refresh the site's page and try again",
  "该网站未登录或登录已过期": "Not signed in to the site, or the session has expired",
  "该网站接口已变化，已停止操作，等待插件更新": "The site's interface has changed; the action was stopped. Waiting for an extension update.",
  "请求过于频繁，稍后再试": "Too many requests; try again later",
  "不是当前登录的账号，已跳过": "Not the currently signed-in account; skipped",
  "已中止": "Aborted",
  "对话不存在": "Conversation not found",
  "网络或网站错误": "Network or site error",
  "正文为空，未删除": "Conversation body is empty; not deleted",
  "Stacker 没有响应，请稍后再试": "Stacker did not respond; try again later",
  "导出文件名无效": "Invalid export file name",
  "Stacker 无法写入它的数据目录": "Stacker cannot write to its data folder",
  "Stacker 拒绝了这个请求": "Stacker refused the request",

  // manage/SyncStatus.tsx, manage/App.tsx (Stacker)
  "未连接 Stacker": "Not connected to Stacker",
  "重新连接": "Reconnect",
  "已连接 Stacker": "Connected to Stacker",
  "待同步": "pending:",
  "项": "item(s)",
  "从 Stacker 恢复": "Restore from Stacker",
  "正在从 Stacker 恢复": "Restoring from Stacker",
  "从 Stacker 恢复账号备注名、文件夹、标签、收藏、备注和摘录？这台浏览器里较新的修改会保留。": "Restore account aliases, folders, tags, favorites, notes and excerpts from Stacker? Newer changes in this browser are kept.",
  "已恢复": "Restored",
  "账号": "Accounts",
  "文件夹": "Folders",
  "对话": "Conversations",
  "条到 Stacker 的导出目录": "item(s) to Stacker's export folder",
};

/** Set from the pages' language preference; null means follow the browser. */
let forced: "zh" | "en" | null = null;

export function setLanguage(lang: "auto" | "zh" | "en"): void {
  forced = lang === "auto" ? null : lang;
}

export function englishUi(): boolean {
  if (forced) return forced === "en";
  return typeof navigator !== "undefined" && !navigator.language.toLowerCase().startsWith("zh");
}

export function t(text: string): string {
  return englishUi() ? EN[text] ?? text : text;
}
