/** English for every Chinese UI string; Chinese is the source text. */
export const EN: Record<string, string> = {
  "Stacker 网页对话": "Stacker Web Chats",

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
  "没有对话。先打开并登录 ChatGPT 或 Claude，再点「刷新」。": "No conversations. Open and sign in to ChatGPT or Claude, then click “Refresh”.",
  "选择一条对话查看详情": "Select a conversation to see its details",

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
  "保存完整 Markdown 和原始数据，再删除网站上的对话。": "Save full Markdown and raw data, then delete the conversation on the site.",
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
  "在 ChatGPT 或 Claude 打开一条对话后，这里会显示它。": "Open a conversation on ChatGPT or Claude to see it here.",
  "这条对话还不在列表里：请在管理页刷新该站点。": "This conversation isn't in the list yet: refresh that site on the manage page.",
  "在网页上选中文字，点出现的「存为摘录」按钮即可添加。": "Select text on the page and click the “Save as excerpt” button that appears to add one.",
  "在管理页打开": "Open in manage page",

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
};

export function t(text: string): string {
  const english = typeof navigator !== "undefined" && !navigator.language.toLowerCase().startsWith("zh");
  return english ? EN[text] ?? text : text;
}
