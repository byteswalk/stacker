# 智能体管理：会话数据

「会话数据」页直接读取 Codex 和 Claude 自己的会话元数据，列出你在两个客户端里实际看到的会话，按项目归类，并支持批量删除。Stacker 不再维护会话全量索引。

## 数据来源

| 智能体 | 根目录 | 读取内容 |
| --- | --- | --- |
| Codex | 「数据来源」中的自定义路径 → 用户环境变量 `CODEX_HOME` → `~/.codex` | 只读打开最新的 `state_<n>.sqlite`：`threads`、`thread_spawn_edges`、`projects`、`project_roots`；正文读取 rollout JSONL |
| Claude | 自定义路径 → `CLAUDE_CONFIG_DIR` → `~/.claude` | `projects/<项目>/<会话>.jsonl` 的开头 256 行（1 MB）和末尾 256 KB；`<会话>/subagents/`、`custom-title.json`、`file-history/<会话>`、`session-env/<会话>` 只统计大小 |
| Claude 桌面端索引 | 自定义路径 → `%APPDATA%\Claude\claude-code-sessions` | `<账号>/<组织>/local_*.json`：侧栏标题、归档状态、最后活动时间 |

Claude 桌面端的 Code 页与 CLI 共用 `~/.claude/projects`，Codex 桌面端、CLI 与 IDE 共用 `~/.codex`，因此按智能体而不是按客户端区分来源。每条会话带一个客户端标签：桌面端、终端、IDE、自动化、SDK。

列表结果在内存中缓存 10 秒；点击「刷新」或修改数据来源后重新读取。

## 分类

- 标题优先级：客户端标题（Claude 桌面端侧栏标题；Codex 重命名后的 `name`，否则首条消息）→ Claude `custom-title` → `summary` → 首条用户消息（80 字）。
- Codex 子智能体与审查线程挂到最上层父会话的「子任务」下，不单独成行，也不参与批量选择。找不到父会话的子线程与审查线程归为自动化运行，并标注「父会话已不存在」。
- Codex `codex exec` 运行与 Claude SDK 会话属于自动化运行，默认隐藏，勾选「包含自动化运行」后显示。
- 状态：
  - 已归档：Codex `archived = 1`，或 Claude 桌面端索引 `isArchived`。
  - 孤儿：Claude 桌面端创建、但已不在桌面端侧栏中的会话（在桌面端删除后文件残留）；或项目目录已不存在。
  - 其余为进行中。
- 项目：按工作目录归类（忽略大小写和尾部分隔符）。Claude worktree 会话（`<仓库>\.claude\worktrees\<名称>`）归到主仓库。Codex 会话优先使用 Codex 桌面端的项目名。

## 删除

选中会话后点击「删除…」。确认框列出将删除的会话数、子任务数、涉及文件数、可释放空间和不会删除的会话及原因。删除方式：

| 方式 | 行为 |
| --- | --- |
| 精简导出后删除（默认） | 把用户、助手正文和工具调用（每条最多 2000 字）写成 Markdown，存到 `%LOCALAPPDATA%\Stacker\<dev|stable>\conversations\exports\<日期>\<智能体>\<项目>\<标题>-<id>.md`，成功后删除 |
| 直接删除 | 不留副本 |
| 完整备份后删除 | 把全部关联文件复制到同目录下 `backups\<任务 ID>\` 后删除，不释放空间 |

安全规则：

- 预览 10 分钟内有效，只能执行一次。执行前重新读取会话并核对文件指纹，有变化即停止。
- 单次最多 500 条。
- Claude：只删除不在桌面端侧栏中的会话。仍在侧栏中的会话会被阻止，请先在 Claude 桌面端删除。最近 2 分钟内有写入的会话视为正在使用并阻止。删除范围为 `projects/<项目>/<会话>.jsonl`、`projects/<项目>/<会话>/`、`file-history/<会话>/`、`session-env/<会话>`。
- Codex：要求 Codex 桌面端与 CLI 已完全退出，并确认本机 Codex 支持安全删除接口；通过 Codex App Server 的 `thread/delete` 先删除子线程再删除父会话，完成后核对记录文件已不存在。
- 不跟随符号链接或目录联接；所有路径必须位于对应智能体根目录内。
- 删除只影响智能体的会话记录，不删除项目源码、工作树或构建缓存。

## 占用

「占用」标签统计 Codex 与 Claude 在磁盘上的全部数据，每一项归入四类：

| 类别 | 内容 | 操作 |
| --- | --- | --- |
| 会话记录 | Codex `sessions`、`archived_sessions`；Claude `projects` | 跳到「会话」标签按占用排序后删除 |
| 可安全清理 | 旧版命令执行器、旧版 Claude Code、临时文件与缓存、7 天前的日志与终端快照、无对应会话的环境快照与编辑检查点 | 默认勾选 |
| 需你判断 | Codex `tmp` 下的工作目录、`target`、生成的图片、附件、可视化产物、运行日志数据库 | 默认不勾选，逐项确认 |
| 保留 | 状态数据库、插件、技能、配置、当前 Claude Code、桌面端组件与运行时 | 只显示大小 |

统计的根目录：Codex 数据目录、`%LOCALAPPDATA%\OpenAI\Codex`、Claude 数据目录、`%APPDATA%\Claude`、`%LOCALAPPDATA%\Claude`、`%LOCALAPPDATA%\Claude-3p` 以及 MSIX 包目录。同一目录经 MSIX 重定向出现两次时只计一次。

清理规则：

- 清理前重新统计，所选项目不存在、被阻止或大小变化超过 1% 时停止。
- 对应程序在运行时，其临时文件、缓存和日志数据库被阻止，提示先退出；Stacker 不会关闭任何程序。
- 旧版 Claude Code 只在没有进程使用该版本时列为可清理；版本号最大的始终保留。
- 不跟随链接或目录联接，删除路径必须位于其根目录内且不能是根目录本身。

## Stacker 保存的数据

`%LOCALAPPDATA%\Stacker\<dev|stable>\conversations\sessions.sqlite3`：

- `session_notes`：收藏、旧版本生成的摘要及其对应的文件指纹（原文变化后摘要标记为已过期）。
- `settings`：数据来源自定义路径。

首次打开时，旧会话索引 `index.sqlite3` 中的收藏与摘要按会话 ID 迁移一次；旧的隐藏、分组标注和 Claude Desktop (local)、导入来源不再使用。
