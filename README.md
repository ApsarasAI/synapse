# Synapse

Synapse 是一个面向 AI Agent 的轻量级代码执行沙箱，目标是提供毫秒级启动与可控隔离。

## Workspace

- `crates/synapse-core`: 核心领域模型与执行抽象
- `crates/synapse-api`: HTTP API 层（axum）
- `crates/synapse-cli`: CLI 入口

## 快速开始

```bash
cargo run -p synapse-cli -- serve --listen 127.0.0.1:8080
```

Linux 环境需要可用的 `bwrap`，否则服务会拒绝启动执行路径。

健康检查：

```bash
curl http://127.0.0.1:8080/health
```
