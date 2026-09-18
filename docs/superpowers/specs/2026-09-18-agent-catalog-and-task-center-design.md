# 智能体目录重建与更新任务中心（子项目 A+B）

日期：2026-09-18
状态：设计已确认，待实现

## 背景

「AI 办公智能体」页面随厂商增加不断打补丁，已出现以下结构性问题：

- 智能体信息分散在 7 处以上：`vibe.rs` 的 `tool_metadata`、`tool_specs`、进程识别 PowerShell 脚本、十余处 `match spec.id` 特例，`work_session.rs` 的 `agent_data_roots`，`space_analysis/walker.rs`，前端 `Vibe.tsx` 图标表与 `DevelopmentProjects.tsx`。新增厂商需要改遍，qoder-cn、trae-global 进程匹配失效即由此导致。
- 安装进度共用 `vibe-progress` 事件，取消共用 `installer::OP_CANCEL` 全局标志；前端 `BusyProvider` 一次只允许一个模态操作。无法并行更新，也无法后台运行。
- 已安装判断只看文件是否存在。曾出现 npm 安装失败留下 500 字节占位 `claude.exe`，被误判为已安装，并在 PATH 中排在正常 WinGet 版本之前。
- 更新器从用户环境变量 `HTTP_PROXY` 推导 `--proxy` 传给 WinGet，外部残留的失效代理导致更新失败。
- WorkBuddy 只有一张「通用」卡且指向国际站；Qoder 未区分新产品线与 Qoder IDE；缺少 pi。

本子项目重建智能体目录与更新执行机制，是后续「会话与数据」（C）、「存储迁移与交接」（D）和代理重设计（E）的数据基础。

## 范围

包含：

1. 智能体注册表与模块拆分。
2. 安装健康检查。
3. 目录调整：WorkBuddy 国内/国际、Qoder 新产品线、新增 pi。
4. 侧栏「智能体」分组与菜单改名；删除受管工作会话。
5. 更新任务中心：多任务、后台执行、一键更新、Toast。
6. 更新器代理注入规则。

不包含：会话层级与批量删除（C）、存储位置迁移与交接包（D）、代理页面与「不干预」模式重设计（E）、本机智能体执行器 AgentRunner（随 C 实现）。

## 1. 智能体注册表

### 模块结构

`src-tauri/src/vibe.rs` 拆分为 `src-tauri/src/agents/`：

| 文件 | 职责 |
| --- | --- |
| `registry.rs` | 纯声明式数据，不含 IO |
| `detect.rs` | CLI 与桌面端检测、版本读取、健康检查 |
| `install/npm.rs` | npm 全局安装、更新、卸载 |
| `install/winget.rs` | WinGet 安装、更新、卸载、可用更新查询 |
| `install/direct.rs` | 官方直链安装包下载、签名校验、静默安装 |
| `install/vendor.rs` | 厂商专有流程（Claude 原生安装、Codex 官方安装器、Kimi、TRAE CLI、OpenCode、OpenClaw、Hermes、Antigravity、DeepSeek Harness 等） |
| `tasks.rs` | 任务管理器与调度（见第 5 节） |
| `activity.rs` | 运行中智能体进程识别 |
| `commands.rs` | Tauri 命令入口 |

所有现有厂商的安装、更新、卸载行为按原逻辑迁移，不改变已验证的安装路径；只改变组织方式与调用入口。

### 数据模型

```rust
pub struct Product {
    pub id: &'static str,            // 唯一，如 "workbuddy-cn"
    pub family: &'static str,        // 同厂商分组，如 "workbuddy"
    pub edition: Edition,            // Cn | Global | Unified
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,          // public/brands 下的文件名
    pub docs_url: &'static str,
    pub sort: u16,
    pub cli: Option<&'static str>,   // 引用 CliSurface.id，可被多个产品共享
    pub desktop: Option<DesktopSurface>,
    pub cli_note: Option<&'static str>, // 如「与国际版共用，登录时选择中国站」
    pub data_dirs: &'static [DataDir],  // C/D 子项目使用；本期只登记
    pub process: ProcessSignature,
}

pub struct CliSurface {
    pub id: &'static str,             // 如 "codebuddy"
    pub name: &'static str,
    pub command: &'static str,
    pub candidates: &'static [&'static str],
    pub strategy: InstallStrategy,
}

pub struct DesktopSurface {
    pub name: &'static str,
    pub detect: DesktopDetect,        // winget id/source、appx 名、注册表关键词、排除词、候选路径
    pub strategy: InstallStrategy,
}

pub enum InstallStrategy {
    Npm { package: &'static str },
    Winget { id: &'static str, source: Option<&'static str> },
    DirectInstaller { url: &'static str, file_name: &'static str, silent_args: &'static [&'static str] },
    Vendor(VendorFlow),               // 厂商专有流程，枚举值对应 install/vendor.rs 中的实现
    ManualDownload { url: &'static str }, // 只能打开官网，不参与一键更新
}
```

`ProcessSignature` 列出可执行文件名与命令行特征；进程识别脚本由注册表生成，短名称必须是完整路径段或可执行文件名。区域版本共享同一个进程特征族。

### 替换的硬编码

- `vibe.rs`：`tool_metadata`、`tool_specs`、PowerShell 进程模式、`activity_family`、`match spec.id` 特例。
- `work_session.rs`：随受管工作会话一并删除。
- `space_analysis/walker.rs` 的智能体痕迹目录名、`space_analysis/monitor.rs` 的运行中智能体：改读注册表。
- 前端 `TOOL_BRAND_ICONS`、`DevelopmentProjects.tsx` 的智能体表：改用后端返回的 `icon`、`name`。

### 注册表不变量（单元测试）

- 产品 id、CLI id 唯一。
- 产品引用的 CLI id 必须存在。
- 每个产品至少有一种使用方式，且图标文件存在于 `public/brands`。
- 同一 `family` 下各产品的 `edition` 不重复。

## 2. 健康检查

检测结果从「是否存在」改为三态：

| 状态 | 条件 |
| --- | --- |
| `healthy` | 找到入口且能在超时内执行 `--version`（或桌面端读取到有效版本资源） |
| `broken` | 找到入口但无法执行：`.exe` 缺少 `MZ` 文件头、执行失败、超时 |
| `missing` | 未找到 |

规则：

- 按 PATH 顺序枚举全部候选入口。`effective` 是 PATH 中第一个；如果它是 `broken` 而后面有 `healthy`，显示「生效入口已损坏，另有健康安装」，并给出修复动作（卸载损坏安装或调整来源），修复动作需用户确认。
- 返回 `other_installs: Vec<InstallInfo>`，界面显示「另有 N 个安装」。
- `broken` 显示原因。`broken` 不可视为已安装，不参与一键更新。

## 3. 目录调整

实现时逐项到官方页面核实安装包 ID、下载地址和进程名，不凭记忆写入；无法核实的使用方式标为 `ManualDownload`。

- **WorkBuddy 中国版**（`workbuddy-cn`）：桌面端来源 workbuddy.cn；CLI 引用共享的 `codebuddy`，附说明「与国际版共用 CodeBuddy CLI，登录时选择中国站」。
- **WorkBuddy 国际版**（`workbuddy-global`）：桌面端来源 workbuddy.ai；CLI 引用 `codebuddy`。
- 共享 CLI 在任一卡片上安装或更新后，两张卡片同时刷新状态。一键更新中共享 CLI 只出现一次。
- **Qoder 国际版 / 中国版**：只保留新 Qoder 桌面端与各自 CLI；桌面检测排除 Qoder IDE；移除 QoderWork。
- **pi**（`pi`）：仅 CLI，`Npm { package: "@mariozechner/pi-coding-agent" }`，命令 `pi`，文档 pi.dev。

## 4. 菜单与删除项

侧栏新增「智能体」分组：

| 页面 id | 菜单名 | 内容 |
| --- | --- | --- |
| `agents` | 安装与更新 | 发现、安装、更新、卸载、修复、打开、一键更新 |
| `agent-data` | 会话与数据 | 现有会话管理（本期不重构，由 C 子项目重建） |

旧页面 id `vibe`、`agent-space` 在页面状态恢复时映射到新 id。

删除受管工作会话：`src/features/agent-workspace/`、`src-tauri/src/work_session.rs`、对应 IPC 命令、`lib.rs` 注册、i18n 文案与 `AgentSpace.tsx` 中的高级区。会话页面的已有报告数据不迁移，直接忽略。

## 5. 更新任务中心

### 任务模型

```rust
pub struct AgentTask {
    pub id: String,
    pub product_id: String,
    pub surface: Surface,             // Cli | Desktop
    pub action: Action,               // Install | Update | Uninstall | Repair
    pub state: TaskState,             // Queued | Running | Succeeded | Failed | Cancelled
    pub message: Option<String>,      // 结果或失败原因
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}
```

每个任务保留最近 200 行日志。状态变化通过 `agent-task` 事件推送完整快照；日志通过 `agent_task_log(id)` 按需读取。完成的任务保留最近 50 个。

命令：`agent_task_start`、`agent_task_cancel`、`agent_task_retry`、`agent_tasks`、`agent_task_log`、`agent_update_plan`、`agent_update_all`。

### 调度规则

任务启动前声明占用的资源，资源被占用则排队：

| 资源 | 适用 |
| --- | --- |
| `npm:<全局前缀目录>` | npm 安装、更新、卸载 |
| `installer` | WinGet、MSI、官方直链安装器 |
| `product:<产品 id 或共享 CLI id>` | 同一产品的 CLI 与桌面端、共享 CLI |
| 下载并发上限 3 | 所有涉及下载的任务 |

调度函数为纯函数：输入排队任务与已占用资源，输出可启动任务。单元测试覆盖冲突、排队顺序和释放后唤醒。

### 任务上下文

每个任务在独立线程运行，并设置线程局部的任务上下文（取消令牌与日志接收器）。

- `installer::op_cancelled()` 优先读取当前任务的取消令牌，没有任务上下文时回退到全局标志。
- `emit_progress` 在有任务上下文时写入该任务日志并触发节流后的状态推送。
- Java、Python 等其他生态页面继续使用全局机制，行为不变。
- 取消时终止该任务的子进程树，不影响其他任务。

### 完成后复检

动作成功后对该产品重新执行健康检查：

- 更新：版本未变化或状态不是 `healthy`，任务记为失败，原因「更新后校验未通过」。
- 安装：状态必须为 `healthy`。
- 卸载：入口必须消失。

### 一键更新

`agent_update_plan` 返回全部 `update_available` 的使用方式，分为两组：

- 可自动更新：执行。
- 需手动处理：`ManualDownload`、`broken`、没有自动更新方式，逐项列出原因，不创建任务。

用户在计划弹窗确认后，`agent_update_all` 为第一组各创建一个任务，交给调度器排队。

### 代理注入

安装与更新只在用户明确设置了 Stacker 手动代理时才传 `--proxy` 或设置下载代理；其余情况不注入任何代理，子进程继承系统环境。删除从用户环境变量 `HTTP_PROXY` 推导 `--proxy` 的逻辑。

### 前端

- 新增 `src/features/agent-tasks/`：任务状态管理（订阅 `agent-task` 事件）、页头任务按钮与任务面板、全局完成 Toast。
- 页头任务按钮显示进行中数量。面板列出每个任务的状态、进度、取消、重试和日志。
- 卡片动作直接创建任务，卡片内显示该使用方式的任务状态，不再使用 `BusyProvider` 模态框。
- 卸载仍需确认弹窗；确认后同样以任务执行。
- 任务完成在任意页面弹 Toast：成功为 ok，失败为 err 并提示可在任务面板查看日志。
- 「安装与更新」页顶部新增「一键更新」按钮，点击显示更新计划弹窗。

## 错误处理

- 任务线程 panic：捕获后任务记为失败，原因「内部错误」，写入日志。
- 应用退出时仍有运行中任务：沿用托盘驻留逻辑；选择退出时提示有未完成任务，确认后终止。
- 厂商安装器返回失败但复检健康：记为成功，并在日志中注明安装器返回值。

## 测试

Rust：

- 注册表不变量。
- 健康检查：`MZ` 头检查、无法执行、超时、多候选选择（使用临时目录中的伪造入口）。
- 调度函数。
- 任务状态流转：用假执行器覆盖成功、失败、取消、复检失败。
- 进程识别模式由注册表生成后的匹配样例。

前端：

- 任务状态管理：事件合并、完成 Toast 触发一次。
- 一键更新计划分组与共享 CLI 去重。

手动验收：

- 同时更新 2～3 个智能体，互不串台；取消其中一个不影响其他。
- 更新过程中切换页面，完成后收到 Toast。
- WorkBuddy 两张卡共享 CLI 状态同步。
- 损坏安装显示为 `broken` 并给出修复入口。

## 实现时需核实

- WorkBuddy 中国版、国际版桌面端安装包来源与进程名。
- 新 Qoder 国际版、中国版桌面端与 CLI 的安装方式、进程名，以及 Qoder IDE 的排除特征。
- pi 的版本命令输出格式。
