# 会话目录重建（子项目 C1）

日期：2026-09-18
状态：已实现

## 背景

「会话数据」页现在列出 1052 条记录，而用户在两个客户端里实际看到的会话约为 Claude 31 条、Codex 70 条。原因：

- 自建全量索引把每个 JSONL 都当成会话：Claude 的 599 个子智能体记录、Codex 的 247 次 `codex exec` 自动化运行和 352 个子智能体 / 审查线程都被平铺出来。
- 标题取第一条用户消息，不读客户端自己的标题（Claude 桌面端索引与 `custom-title`，Codex `threads.title` / `name`）。
- 「Claude Desktop (local)」来源读取 `local-agent-mode-sessions`，该目录没有任何会话，只有插件与定时任务配置。Claude 桌面端 Code 页与 CLI 共用 `~/.claude/projects`；Codex 桌面端、CLI、IDE 共用 `~/.codex`。按客户端拆来源是错误建模。
- 客户端的归档、置顶、项目信息没有使用；在 Claude 桌面端删除的会话（桌面端索引留下 `deleted_*` 标记）其记录文件仍在磁盘上，页面无法识别。
- 批量删除先完整备份原始 JSONL，删除后不释放空间；Claude 会话不能删除。

本子项目重建会话数据层与页面。磁盘足迹账本（C2）与本机智能体执行器（C3）不在范围内。

## 已核实的本机数据格式

Claude（根目录 `CLAUDE_CONFIG_DIR` 或 `~/.claude`）：

| 位置 | 内容 |
| --- | --- |
| `projects/<项目 slug>/<sessionId>.jsonl` | 主会话记录；记录行含 `sessionId`、`cwd`、`gitBranch`、`entrypoint`（`claude-desktop` / `cli` / `sdk-cli`）、`type`（含 `custom-title`、`summary`、`worktree-state`、`relocated`） |
| `projects/<slug>/<sessionId>/subagents/agent-*.jsonl` | 子智能体记录 |
| `projects/<slug>/<sessionId>/custom-title.json` | `{"customTitle": "..."}` |
| `file-history/<sessionId>/` | 编辑检查点 |
| `session-env/<sessionId>` | 会话环境快照 |
| `%APPDATA%\Claude\claude-code-sessions\<account>\<org>\local_<id>.json` | 桌面端索引：`cliSessionId`、`title`、`isArchived`、`cwd`、`lastActivityAt`、`createdAt` |
| 同目录 `deleted_<id>` | 桌面端删除标记（仅含时间戳，无法映射到 `cliSessionId`） |

Codex（根目录 `CODEX_HOME` 或 `~/.codex`）：

| 位置 | 内容 |
| --- | --- |
| `state_<n>.sqlite` 表 `threads` | `id`、`rollout_path`、`title`、`name`、`archived`、`is_pinned`、`cwd`、`source`（`vscode` / `cli` / `exec` / JSON `subagent`）、`thread_source`（`user` / `subagent` / `guardian_review`）、`originator`（如 `Codex Desktop`）、`project_id`、`created_at`、`updated_at` |
| 表 `thread_spawn_edges` | 父子线程关系 |
| 表 `projects`、`project_roots` | Codex 桌面端项目名称与根路径 |
| `sessions/`、`archived_sessions/` 下的 rollout JSONL | 会话正文 |

## 1. 数据层：直接读取客户端元数据

不再维护会话全量索引。每次列出时：

- **Codex**：只读打开最新的 `state_<n>.sqlite`，从 `threads` 取列表字段，`thread_spawn_edges` 与 `source` 中的 `parent_thread_id` 取父子关系，`projects`/`project_roots` 取项目名。正文与原文搜索仍读 rollout 文件。
- **Claude**：读取桌面端索引全部 `local_*.json`；枚举 `projects/*/*.jsonl`，每个文件只读取开头若干行（上限 256 行 / 1 MB）得到 `sessionId`、`cwd`、`entrypoint`、首条用户消息、`custom-title`、`summary`、`worktree-state`；子智能体与关联目录只统计数量与大小。
- 结果按文件指纹（大小 + 修改时间）在内存缓存，未变化的文件不重复读取。
- Stacker 自己的数据只保留在小表 `annotations`（收藏、摘要及其指纹），键为 `agent:nativeId`。旧索引表停用，旧注释按键迁移。

### 会话模型

```rust
pub struct Session {
    pub id: String,              // "codex:<threadId>" / "claude:<sessionId>"
    pub agent: Agent,            // Codex | Claude
    pub native_id: String,
    pub title: String,
    pub title_source: TitleSource, // Client | Custom | Summary | FirstMessage
    pub project: ProjectRef,     // 见第 2 节
    pub client: ClientTag,       // Desktop | Terminal | Ide | Automation | Sdk | Unknown
    pub created_at: u64,
    pub updated_at: u64,
    pub archived: bool,
    pub pinned: bool,
    pub status: SessionStatus,   // Active | Archived | Orphaned
    pub children: Vec<ChildSummary>, // 子智能体 / 审查线程：id、类型、大小
    pub bytes: u64,              // 会话自身 + 子会话 + 关联目录
    pub path: String,
    pub favorite: bool,
    pub summary: Option<String>,
    pub summary_stale: bool,
}
```

分类规则：

- Codex：`thread_source = user` 且 `source` 不是 `exec` → 普通会话；`source = exec` → `Automation`；`thread_source` 为 `subagent` / `guardian_review` → 挂到父线程的 `children`。找不到父线程的子线程与审查线程归为 `Automation`（默认隐藏），并标记「父会话已不存在」。
- Codex 客户端标签：`originator = Codex Desktop` → Desktop；`source = vscode` 且非桌面 → Ide；`source = cli` → Terminal；`exec` → Automation。
- Claude：`entrypoint = claude-desktop` → Desktop；`cli` → Terminal；`sdk-cli` / `sdk-*` → Sdk（按自动化运行处理）。`subagents/` 下的记录只作为 `children`。
- 标题优先级：客户端标题（Claude 桌面端索引 `title`，Codex `name` 再 `title`）→ `custom-title` → `summary` → 首条用户消息（截取 80 字）。

### 状态

- `Archived`：Codex `archived = 1`；Claude 桌面端索引 `isArchived = true`。
- `Orphaned`（孤儿）：
  - Claude：`entrypoint = claude-desktop` 但桌面端索引中没有对应 `cliSessionId`（用户已在桌面端删除，文件残留）。
  - 两者：项目目录已不存在。
- 其余为 `Active`。

## 2. 项目

`ProjectRef { key, name, path, exists }`：

- 项目键为规范化后的工作目录（小写、去掉尾部分隔符）。
- Claude worktree 会话（路径含 `\.claude\worktrees\<name>` 或有 `worktree-state` / `relocated` 记录）归到主仓库路径。
- 名称：Codex 会话所在路径落在 `project_roots` 中时用 Codex 项目名；否则用目录名。
- `exists` 表示路径当前是否存在。

## 3. 页面

「会话数据」页三个标签：

- **会话**：筛选项为智能体（全部 / Codex / Claude）、项目、状态（进行中 / 已归档 / 孤儿）、时间、客户端标签；「包含自动化运行」开关默认关闭。列表每行：收藏星标、标题、项目、客户端标签、子任务数、状态、最后活动、占用。选中后底部固定批量操作条。子任务可展开查看，不参与批量选择。
- **项目**：每行项目名、路径（不存在时标注「已删除」）、智能体、会话数、孤儿数、占用、最后活动；点击进入该项目的会话筛选。
- **数据来源**：Codex 与 Claude 两个根目录（尊重 `CODEX_HOME` / `CLAUDE_CONFIG_DIR`，可改为自定义路径）以及 Stacker 导出目录。移除「Claude Desktop (local)」与导入来源。

收藏图标修复：未收藏 `ti-star`，收藏后仍用 `ti-star` 加填充色类名，不使用字体中不存在的 `ti-star-filled`。

## 4. 批量删除

确认框列出：会话数、子会话数、涉及文件数、可释放空间、被阻止项及原因，以及删除方式：

| 方式 | 行为 |
| --- | --- |
| 精简导出后删除（默认） | 把可读正文（用户、助手文本与工具调用摘要；不含压缩快照与图片数据）写成 Markdown，存到 Stacker 数据目录 `exports/<日期>/<智能体>/<项目>/<标题>-<id>.md`，成功后删除 |
| 直接删除 | 不留备份 |
| 完整备份后删除 | 复制全部关联文件到 `backups/<操作 ID>/` 后删除（旧行为） |

执行规则：

- 预览有效 10 分钟、只能执行一次；执行前重新计算影响范围与文件指纹，变化即停止。
- **Codex**：要求 Codex 桌面端与 CLI 已完全退出；通过 Codex App Server 官方接口删除，子线程先于父线程。
- **Claude**：只允许删除不在桌面端侧栏中的会话——孤儿、`cli` 与 `sdk` 创建的会话。桌面端索引中仍存在的会话显示为被阻止，原因「请先在 Claude 桌面端删除该会话」。删除范围：`projects/<slug>/<id>.jsonl`、`projects/<slug>/<id>/`、`file-history/<id>/`、`session-env/<id>`。会话记录在最近 120 秒内有写入时视为正在使用并阻止（Windows 无法可靠读取其他进程的工作目录）。
- 删除不跟随链接或目录联接；所有路径必须位于对应根目录内。
- 结果以后台任务报告，逐项列出成功、跳过和失败。

## 5. 移除项

- 旧会话全量索引（`conversations` 表、扫描任务、`conversations_start` 的 `scan`）。
- API Key 摘要：模型设置、密钥存储、发送确认与请求代码。已有摘要作为只读内容保留显示。
- 「隐藏」与「自定义分组」注释。
- 「Claude Desktop (local)」与「导入目录」来源。旧导出的 JSON 阅读包不再导入。

## 6. 不在范围内

- 磁盘足迹账本，包括与会话无关的残留（如 `session-env` 中无对应会话的条目）：C2。
- 用本机智能体生成摘要与交接资料：C3。
- 存储位置迁移：D。
- Claude 桌面端侧栏分组（存于桌面端内部浏览器存储，格式不公开）。

## 测试

Rust（临时目录中构造合成数据）：

- Codex：合成 `state` 数据库覆盖普通会话、`exec`、子线程、审查线程、归档、置顶、项目名；父子挂载与客户端标签。
- Claude：合成 `projects` 与桌面端索引，覆盖 `entrypoint` 分类、子智能体、`custom-title`、`summary`、worktree 归并、孤儿识别。
- 标题优先级、项目键规范化。
- 删除：Claude 删除范围、被阻止规则、链接拒绝、指纹变化中止；精简导出只包含可读正文。

前端：筛选（默认隐藏自动化运行）、批量选择不包含子任务、项目表跳转。

手动验收：本机 Claude 约 31 条、Codex 约 70 条（默认视图），标题与客户端一致；孤儿会话可识别并可删除。
