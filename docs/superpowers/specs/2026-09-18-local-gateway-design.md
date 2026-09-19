# 本机智能体网关（子项目 F）

日期：2026-09-18
状态：已实现

## 目标

把本机已登录的 Codex / Claude（经 C3 的 AgentRunner）以 OpenAI 与 Anthropic 风格的 HTTP 接口提供给用户自己的工具使用。只有对话能力，没有操作电脑的能力。

约束（用户已同意）：只监听 127.0.0.1，只供本机自用；不支持对外共享、账号池化。

## 1. 服务

- 监听 `127.0.0.1:<端口>`（默认 8765，可改）。不提供监听其他地址的选项。
- 默认关闭；在「智能体管理 → 本机网关」页开启。开启状态保存后，Stacker 启动时自动恢复。
- 鉴权：Stacker 生成随机密钥（`sk-stacker-` + 32 位十六进制），请求需带 `Authorization: Bearer <密钥>` 或 `x-api-key: <密钥>`；可一键重新生成。
- 拒绝带 `Origin` 头的请求（浏览器页面），防止网页通过 localhost 调用。
- 并发：同时运行 2 个请求，最多排队 8 个，超过返回 429。
- 单次请求超时 5 分钟。客户端断开时，运行仍会完成后丢弃结果（所用 HTTP 库无法得知连接已断开）。

## 2. 接口

| 方法与路径 | 说明 |
| --- | --- |
| `GET /v1/models` | 列出可用模型（OpenAI 格式） |
| `POST /v1/chat/completions` | OpenAI Chat Completions；支持 `stream` |
| `POST /v1/messages` | Anthropic Messages；支持 `stream` |
| `GET /health` | 无需鉴权，返回 `ok` |

模型名：`codex`、`claude`，或 `codex/<模型>`、`claude/<模型>`（如 `claude/sonnet`、`codex/gpt-5.6-sol`）。推理强度：OpenAI 请求的 `reasoning_effort`；未提供时使用 C3 摘要设置中该智能体的推理强度。

对话渲染：执行器是无状态的，每次请求把 `system` 与全部消息按顺序渲染为一段对话记录，要求模型以助手身份回复最后一条用户消息。仅支持文本；包含图片或 `tools` / 函数调用的请求返回 400（`unsupported`）。

流式：执行器只返回完整结果，`stream: true` 时以一个内容块加结束事件的 SSE 返回（OpenAI：`chat.completion.chunk` + `[DONE]`；Anthropic：`message_start` … `message_stop`）。

`usage`：按字符数估算（约 4 字符 1 token），字段齐全以兼容客户端。

## 3. 页面「本机网关」

- 开关、端口、运行状态、接口地址（OpenAI base URL `http://127.0.0.1:<端口>/v1`、Anthropic base URL `http://127.0.0.1:<端口>`）、密钥（显示 / 复制 / 重新生成）、可用模型列表。
- 调用示例（curl、OpenAI SDK、Anthropic SDK）一键复制。
- 最近请求：时间、接口、模型、耗时、结果码（不记录内容）。
- 说明：仅供本机自用；调用会消耗你在对应智能体中登录的账号额度；请勿把端口或密钥提供给他人。

## 4. 不在范围内

工具调用、图片输入、真实逐字流式、对外监听、多用户。

## 测试

协议解析与渲染（OpenAI / Anthropic、system、多轮、拒绝 tools 与图片）、模型名解析、SSE 格式、鉴权与 Origin 拒绝、并发上限；使用假执行器的端到端 HTTP 测试（真实端口 127.0.0.1:0）；手动：用 curl 分别以两种风格调用 codex 与 claude。
