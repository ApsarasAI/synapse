# Synapse

[English](README.md)

Synapse 是一个面向 Linux 的轻量级代码执行沙箱，提供受控隔离、审计记录，以及尽量小的 HTTP/CLI 接口，适合做 AI Agent 代码执行能力的开发者试用版集成。

当前仓库的目标不是“展示可运行代码”，而是“让外部开发者 30 分钟内能完成主机检查、导入 runtime、启动服务、发起一次执行、查看审计并理解如何贡献”。

## 项目价值

- 在受限沙箱内执行不受信任或半受信任的 Python 代码片段。
- 通过 pooled sandbox 降低启动开销。
- 暴露最小但够用的控制面：`doctor`、runtime 管理、`/execute`、`/audits/:request_id`、`/metrics`。
- 把宿主机依赖显式化，避免隐含前提。

## 适用场景

- Agent 工具执行链路的本地集成验证。
- 需要可审计代码执行能力的内部开发者平台。
- 围绕沙箱策略、runtime 制品、可观测性的安全/系统实验。

## 版本与兼容承诺

- 版本策略：遵循 SemVer。
- 稳定性声明：当前为 `0.x` 开发者试用版，minor 版本之间仍可能有破坏性变更。
- 当前相对稳定的接口：
  - `synapse doctor`
  - `synapse runtime list|verify|install|install-bundle|import-host|activate`
  - `GET /health`
- 在 `0.x` 中仍可能变化的部分：
  - `POST /execute` 的请求/响应字段
  - `GET /audits/:request_id` 的事件字段
  - `GET /metrics` 的指标名和维度
  - streaming 执行接口行为
- 平台支持：
  - Linux：首发支持的安全沙箱路径
  - macOS / Windows：不在首发承诺范围内，编译成功不代表具备同等隔离保证

## 系统要求

安全执行路径当前要求 Linux 主机具备：

- `bwrap` / bubblewrap 可在 `PATH` 中找到
- `strace` 可在 `PATH` 中找到
- cgroup v2，且 `cpu`、`memory`、`pids` 控制器可写
- 如果使用 `synapse runtime import-host`，还需要 `python3`
- 若从源码构建，需要 Rust stable

先执行自检：

```bash
cargo run -p synapse-cli -- doctor
```

常见失败项：

- `sandbox` 失败：安装 `bubblewrap`，并启用 unprivileged user namespaces。
- `strace` 失败：安装 `strace`。
- `cgroupv2` 失败：挂载可写的 cgroup v2，并暴露 `cpu`、`memory`、`pids`。
- `runtime` 失败：先导入或安装一个 managed runtime。

## 安装方式

推荐优先级：

1. Rust 开发者

```bash
cargo install --path crates/synapse-cli --locked
synapse --help
```

2. 非 Rust 用户

- 从 GitHub Releases 下载 Linux 二进制
- 校验发布页提供的 SHA-256
- 运行 `./synapse --help` 和 `./synapse doctor`

预期 Release 附件：

- `synapse-linux-x86_64.tar.gz`
- `synapse-linux-x86_64.sha256`
- 变更摘要

发布流程见 [docs/release-process.md](docs/release-process.md)。

## 最小闭环

如果你只想最快跑通：

1. 导入一个 managed runtime

```bash
cargo run -p synapse-cli -- runtime import-host --language python --version system --command python3 --activate
```

2. 运行主机自检

```bash
cargo run -p synapse-cli -- doctor
```

3. 启动服务

```bash
cargo run -p synapse-cli -- serve --listen 127.0.0.1:8080
```

4. 健康检查

```bash
curl http://127.0.0.1:8080/health
```

预期输出：

```text
ok
```

5. 发起第一次执行

```bash
curl \
  -X POST http://127.0.0.1:8080/execute \
  -H 'content-type: application/json' \
  -H 'x-synapse-request-id: hello-demo' \
  -d '{
    "language": "python",
    "code": "print(\"hello from synapse\")\n",
    "timeout_ms": 5000,
    "memory_limit_mb": 128
  }'
```

6. 读取对应审计记录

```bash
curl http://127.0.0.1:8080/audits/hello-demo
```

7. 检查指标

```bash
curl http://127.0.0.1:8080/metrics | rg '^synapse_'
```

完整 10 分钟上手见 [docs/quickstart/10-minute-quickstart.md](docs/quickstart/10-minute-quickstart.md)。

## CLI 与 README 对齐

README 里的命令示例直接对应当前 CLI：

```bash
synapse --help
synapse serve --help
synapse runtime --help
```

核心命令：

- `synapse serve --listen 127.0.0.1:8080`
- `synapse doctor`
- `synapse runtime import-host --activate --language python --version system --command python3`
- `synapse runtime verify --language python`
- `synapse runtime list`

## API 对外接口

当前开发者试用版公开的 HTTP 接口：

- `GET /health`
- `POST /execute`
- `GET /audits/:request_id`
- `GET /metrics`

认证行为：

- 未设置 `SYNAPSE_API_TOKENS` 时，认证关闭。
- 设置后，`/execute`、`/audits/:request_id`、`/metrics`、`/execute/stream` 都要求 `Authorization: Bearer <token>`。

请求/响应/错误码示例见 [docs/api-reference.md](docs/api-reference.md)。

## 常见排查

`synapse doctor` 卡在 `cgroupv2`

- 此时 `/health` 可能仍可用，但不代表安全执行路径已满足首发要求。
- 需要把 cgroup v2 配置成可写，并确认 `cpu`、`memory`、`pids` 控制器存在。

`/execute` 返回 `runtime_unavailable`

- 导入或安装 runtime，然后执行 `synapse runtime verify --language python`。

`/execute` 返回 `sandbox_policy_blocked`

- 当前版本只支持 `network_policy.mode = "disabled"`。

`/execute` 返回 `queue_timeout` 或 `capacity_rejected`

- 调整 pool / tenant queue 配置，或降低并发请求。

`/audits/:request_id` 返回 `404`

- 确认 execute 请求使用了同一个 `request_id`，并且租户上下文一致。

## 参与贡献

先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)、[SECURITY.md](SECURITY.md)、[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。

推荐入口：

- [docs/quickstart/10-minute-quickstart.md](docs/quickstart/10-minute-quickstart.md)
- [docs/api-reference.md](docs/api-reference.md)
- [docs/release-process.md](docs/release-process.md)
