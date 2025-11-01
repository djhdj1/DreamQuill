# DreamQuill

一个采用 Rust + TypeScript 构建的多包（monorepo）项目，包含：
- 核心 SDK（Rust，`packages/core-sdk`）：LLM 服务对接、SQLite 持久化、HTTP API。
- CLI（Rust，`apps/cli`）：初始化 Provider、发起对话、启动本地 HTTP 服务。
- 桌面应用（Tauri 2，`apps/desktop`）：原生桌面端聊天体验，内置安全存储。
- Web UI（Vite/React，`packages/ui`）：通过 TS SDK 调用 API 或 Tauri 指令。
- TS SDK（`packages/ts-sdk`）：统一传输抽象（HTTP / Tauri），提供 Provider 与 Chat 服务调用。

数据默认持久化到仓库根目录的 `dreamquill.db`（SQLite，WAL 模式）。


## 目录结构

- `packages/core-sdk`：Rust 核心能力（DB、LLM、HTTP server）。
- `apps/cli`：`dreamquill` 命令行工具。
- `apps/desktop`：Tauri 2 桌面应用（打包与运行脚本在此）。
- `packages/ui`：Vite + React 前端（开发端口 5173，代理 `/api` 到 5174）。
- `packages/ts-sdk`：前端/桌面共享的 TypeScript SDK。
- `web`：回退静态资源目录（若未构建 UI，将由 HTTP 服务回落到此）。


## 环境要求

- Node.js ≥ 18（建议 18/20 LTS）
- npm（已配置 workspace，可直接在仓库根目录执行）
- Rust stable（含 Cargo）
- Tauri 2 环境（仅桌面端）：
  - Windows：安装 Visual Studio 生成工具、WebView2 运行时
  - 参考官方文档安装操作系统依赖（此处略，按本机环境补齐）

首次安装依赖：

```bash
npm install
```


## 快速开始

### 方案 A：桌面端（推荐）

无需本地 HTTP 服务，前端通过 Tauri 指令直接调用核心能力。

- 开发运行：
```bash
npm run dev --workspace @dreamquill/desktop
```
- 构建安装包：
```bash
npm run build --workspace @dreamquill/desktop
```

首次启动请在应用内“模型服务”页配置 Provider（见“Provider 配置”）。桌面端会把 API Key 放入安全存储（不落盘 DB）。


### 方案 B：Web + 本地 HTTP 服务

开发（UI 走 Vite，API 走 CLI 提供的 axum 服务）：

1) 启动 API（监听 5174，与 Vite 代理匹配）：
```bash
cargo run -p dreamquill-cli -- serve --addr 127.0.0.1:5174
```
2) 启动前端（Vite dev server：5173，已代理 `/api` → `127.0.0.1:5174`）：
```bash
npm run dev:ui
```
3) 打开浏览器访问：http://127.0.0.1:5173

构建并由后端统一托管静态资源：

1) 构建前端产物：
```bash
npm run build:ui
```
2) 启动服务并托管 UI（默认会优先读取 `packages/ui/dist`）：
```bash
cargo run -p dreamquill-cli -- serve --addr 127.0.0.1:5173
```
3) 打开浏览器访问：http://127.0.0.1:5173

可选环境变量（启动 `serve` 前设置）：
- `DREAMQUILL_UI_DIR`：静态 UI 根目录（默认 `packages/ui/dist`）
- `DREAMQUILL_UI_FALLBACK`：回退目录（默认 `web`）


### 方案 C：CLI 最小可用

初始化 Provider 并对话（shell 示例）：

```bash
# 1) 初始化默认 Provider（以 OpenAI 为例）
cargo run -p dreamquill-cli -- \
  init \
  --name default \
  --provider openai \
  --api-base https://api.openai.com/v1 \
  --api-key sk-xxxx \
  --model gpt-4o \
  --enable-telemetry=false

# 2) 发送一条消息并流式输出助手回复
cargo run -p dreamquill-cli -- chat --prompt "你好，DreamQuill" 
```


## Provider 配置

无论桌面端、Web 还是 CLI，核心需要配置一条可用的 LLM Provider：
- `name`：自定义名称
- `provider`：服务类型（如 `openai`）
- `api_base`：接口基本地址（OpenAI 为 `https://api.openai.com/v1`）
- `api_key`：访问密钥
- `model`：默认模型名称（如 `gpt-4o`/`gpt-4o-mini` 等）
- `telemetry_enabled`：是否上报匿名事件（默认 false，可在 UI 或接口关闭）

桌面端：API Key 存于安全存储；HTTP 服务模式下 Key 存于本地 SQLite。


## 数据与存储

- SQLite 文件位于仓库根目录：`dreamquill.db`，开启 WAL，会看到 `*.db-wal`、`*.db-shm`。
- 若要重置数据，关闭应用后删除这些文件即可（请先确认无重要数据）。


## 常用脚本速查（根目录）

```bash
# 前端开发 / 构建
npm run dev:ui
npm run build:ui

# 桌面端开发 / 构建
npm run dev --workspace @dreamquill/desktop
npm run build --workspace @dreamquill/desktop

# 启动本地 API（含静态托管能力）
cargo run -p dreamquill-cli -- serve --addr 127.0.0.1:5173
```


## 可能的问题

- 端口冲突：
  - Vite 默认 5173；HTTP API（开发态）请使用 5174，并由 Vite 代理 `/api`（已在 `packages/ui/vite.config.ts` 配置）。
  - 一体托管（生产/演示）时可用 `5173` 并直接打开 HTTP 服务地址。
- Tauri 依赖：
  - Windows 需安装 WebView2 运行时与 VS 构建工具；若缺少依赖，`npm run dev --workspace @dreamquill/desktop` 会报错，请按提示补齐。


## 合并到 master 分支（Git 操作示例）

若你当前在开发分支（例如 `docs/readme`），可按如下操作合并到 `master`：

```bash
# 确认处于仓库根目录
# 1) 新建并切到文档分支（若已在分支可跳过）
git switch -c docs/readme

# 2) 添加并提交 README
git add README.md
git commit -m "docs: add README"

# 3) 切回 master 并合并
git switch master
# 如果 master 不存在，可先创建： git branch -M master
git merge --no-ff docs/readme -m "merge: docs/readme into master"
```

> 如果你使用 PR 工作流，请把分支推到远端后在 GitHub 上发起 PR，完成代码审查后合并。


## 推送到 GitHub（首次）

1) 在 GitHub 创建一个空仓库（例如 `yourname/DreamQuill`）。
2) 本地执行：

```bash
# 初始化仓库（若已有 .git 可跳过）
git init

# 配置 master 为默认分支（如果需要）
git branch -M master

# 添加远端（SSH 或 HTTPS 二选一）
# SSH：
git remote add origin git@github.com:yourname/DreamQuill.git
# HTTPS：
# git remote add origin https://github.com/yourname/DreamQuill.git

# 首次推送（将 master 推上去）
git push -u origin master
```

如远端已存在且需替换地址：
```bash
git remote set-url origin git@github.com:yourname/DreamQuill.git
```

> 推送后即可在 GitHub 查看 README 与分支。后续开发请在分支上提交并通过 PR 合并到 master。


## 许可证

本项目使用 MIT 许可证。

