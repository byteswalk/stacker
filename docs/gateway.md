# 智能体管理：接口服务

「接口服务」把本机已安装并登录的智能体（目前为 Codex、Claude、CodeBuddy、Qoder 国际版与中国版、Kimi Code、MiMo Code、Antigravity）以 OpenAI 与 Anthropic 风格的 HTTP 接口提供给你自己的工具。它复用会话摘要使用的执行器：每次请求在临时空目录中无状态运行，没有任何工具，不能读写文件或执行命令，也不会在智能体中留下会话。

## 接入

| 项 | 值 |
| --- | --- |
| OpenAI base URL | `http://127.0.0.1:<端口>/v1`（默认端口 8765） |
| Anthropic base URL | `http://127.0.0.1:<端口>` |
| 密钥 | 页面显示的 `sk-stacker-…`，放在 `Authorization: Bearer` 或 `x-api-key` |
| 模型 | `codex`、`claude`、`codebuddy`、`qoder`、`qodercn`、`agy`、`kimi`、`mimo`，或 `<智能体>/<模型>`（调用名见页面上各智能体的模型表） |

接口：`GET /v1/models`、`POST /v1/chat/completions`、`POST /v1/messages`、`GET /health`（无需密钥）。

- 只写 `codex` / `claude` 时，使用「会话数据 → 设置 → 摘要」中该智能体的默认模型与推理强度；其他智能体只写名字时使用该 CLI 自己的默认模型；OpenAI 请求的 `reasoning_effort` 可覆盖推理强度。
- 每次请求把 system 与全部消息渲染为一段对话交给执行器，多轮对话由客户端自己携带历史。
- 附件：Codex 可读图片，Claude 可读图片和 PDF（PNG、JPEG、GIF、WebP；OpenAI 的 `image_url`/`input_image` 与 Anthropic 的图片、文档块都可以）；其他智能体收到附件时直接返回错误。带 `tools` 的请求返回 400。
- `stream: true` 是真流式：智能体产生文字就转发；第一段文字到来之前出错，会按普通错误响应返回。
- `usage` 按字符数估算（约 4 字符 1 token）。

## 安全

- 默认只监听 127.0.0.1。打开「允许局域网访问」后改为监听全部网卡，页面会列出本机各网卡地址（标明是网卡、Tailscale 还是虚拟机网卡），也可一键放行 Windows 防火墙端口；局域网请求同样需要密钥。
- 每个请求都需要密钥；带 `Origin` 头的浏览器请求一律拒绝，网页无法通过 localhost 调用。
- 同时运行 2 个请求，最多排队 8 个，其余返回 429；单次 5 分钟超时。
- 请求记录只保存接口、模型、状态码、耗时、客户端、是否流式、推理强度、轮数、附件数、输入输出字数和错误摘要，不保存请求或回复内容；记录可筛选、可清空。
- 单个请求体上限 64 MB。
- 调用消耗你在对应智能体中登录账号的额度。不要把密钥提供给他人；打开局域网访问时只在可信网络中使用，不要通过端口转发暴露到公网。

## 各智能体的接入情况

- **CodeBuddy**：没有查询登录状态的命令，页面显示「登录状态未知」，用「测试」确认是否可用。模型列表由 CLI 按账号返回，每 30 分钟刷新一次。运行后删除它为临时目录写的日志。
- **Qoder**：关闭全部插件与钩子运行（否则每次请求都会执行插件命令）；运行后删除它为临时目录写的事件日志。未知模型名会被 Qoder 自动换成 `Auto`。
- **Antigravity (agy)**：每次运行使用一个临时主目录，其中拒绝全部权限并用钩子拒绝每一次工具调用（否则无头模式仍能读取任意文件、联网搜索）；登录不受影响。运行后删除临时主目录。提问经命令行传入，上限约 3 万字。
- **Qoder 中国版 (qodercn)**：和国际版是两个程序（npm 包 `@qodercn-ai/qoderclicn`，命令 `qodercn`），账号与模型清单都不通用，因此是独立的一张卡片和独立的调用名。
- **Kimi Code (kimi)**：用一个 `tools: []` 的临时智能体定义运行，禁止调用工具；运行目录是临时目录，运行后删除 Kimi 为它建立的会话目录。Kimi 自己决定思考深度，没有推理档位。
- **MiMo Code (mimo)**：用 `mimo run --format json --pure` 运行，不加载外部插件；答案取自事件流里的 `text` 片段，运行后按事件里的会话 id 删除该会话。推理档位 minimal / high / max。登录状态取自 `mimo auth whoami`。注意：授权登录前需要小米开放平台账户有余额或 Token Plan。
- **不接入**：DeepSeek Harness、Hermes、OpenCode、OpenClaw、pi 都要自备第三方 API key；TRAE CLI 官方只向 TRAE 企业版旗舰套餐开放。这些不会出现在接口服务页面上；已安装但未登录的智能体会单独列出来提示登录。
