<!-- Generated: 2026-03-25 | Files scanned: 24 | Token estimate: ~1100 -->
# 系统架构

## 项目类型
- Rust workspace
- 3 个 crate：`synapse-core`、`synapse-api`、`synapse-cli`
- 典型形态：单应用后端 + CLI 包装器
- 无前端子系统

## 当前模块边界
```text
synapse-cli
  -> main.rs      命令解析与装配
  -> doctor.rs    provider 驱动的系统检查

synapse-api
  -> app.rs       AppState 与默认装配
  -> server.rs    路由、状态注入、HTTP 映射

synapse-core
  -> types.rs     请求/响应纯数据结构
  -> error.rs     统一错误模型
  -> config.rs    配置读取与默认值
  -> providers.rs 横切能力入口
  -> service.rs   业务编排与请求校验
  -> runtime.rs   沙箱、脚本、进程与平台执行
  -> pool.rs      预热池、租约与指标
  -> executor.rs  兼容导出层
  -> seccomp.rs   Linux seccomp 支持
```

## 分层依赖
```text
types/error
  -> config
  -> providers
  -> service
  -> runtime
  -> pool
  -> api/cli
```

补充说明：
- `service` 可以调用 `runtime`，但 `runtime` 不能反向依赖 `service`、`api`、`cli`。
- `api` 和 `cli` 是入口层，只负责协议适配、命令装配和启动。
- `executor.rs` 当前只作为兼容 facade，避免外部调用点在拆分期间失稳。

## 执行链路
```text
POST /execute
  -> axum router
  -> AppState.pool()
  -> SandboxPool::execute()
  -> SandboxLease::execute()
  -> service::execute_in_prepared() | service::execute()
  -> runtime::execute_binary()
  -> Python subprocess / bubblewrap sandbox
  -> ExecuteResponse JSON
```

## 关键系统图
```text
Client
  -> synapse-cli serve
  -> synapse-api::server
  -> in-memory SandboxPool
  -> service layer
  -> runtime layer
  -> OS process + temp workspace + seccomp/bwrap
```

## 关键文件
- `crates/synapse-cli/src/main.rs`：CLI 入口、子命令解析、服务装配
- `crates/synapse-cli/src/doctor.rs`：统一 provider 驱动的运行环境检查
- `crates/synapse-api/src/app.rs`：API 默认状态与池装配
- `crates/synapse-api/src/server.rs`：HTTP 路由、状态注入、错误映射、metrics 输出
- `crates/synapse-core/src/service.rs`：请求校验、语言解析、执行编排
- `crates/synapse-core/src/runtime.rs`：沙箱目录、脚本写入、进程执行、超时与平台策略
- `crates/synapse-core/src/pool.rs`：池化复用、租约回收、指标统计
- `crates/synapse-core/src/providers.rs`：`Providers` trait 与系统实现

## 服务边界
- `synapse-cli`：命令解析、启动入口、doctor 检查触发；不承载核心执行规则
- `synapse-api`：HTTP 接口、状态注入、错误映射、metrics；不承载平台探测与业务决策
- `synapse-core`：配置、providers、执行编排、运行时、安全隔离、响应模型

## 架构要求
- 业务代码不得直接读取环境变量、探测 PATH、生成临时目录名或直接依赖进程环境；统一经由 `Providers`。
- `service` 负责“执行什么”和“如何归类结果”，不直接处理 HTTP/CLI 协议。
- `runtime` 负责“如何执行”，包括沙箱目录、脚本写入、子进程启动、超时控制、seccomp/bwrap。
- `api` 只负责路由、状态注入、HTTP 映射和错误到状态码的转换。
- `cli` 只负责命令解析、装配和触发 provider 驱动的检查逻辑。
- Linux 安全执行路径必须依赖 `bwrap`，不允许静默 fallback 到非隔离执行。
- 横切能力必须从单一显式入口 `Providers` 进入，不能新增 ambient 依赖。
- 新增功能时优先放入现有层级，不得把认证、连接器、遥测或功能标志直接塞进 `service`/`runtime` 主链路。

## 禁止事项
- 禁止 `synapse-core` 直接读取 `std::env`。
- 禁止 `api` 绕过 `service` 直接调用 `runtime`。
- 禁止 `runtime` 处理 HTTP 状态码、JSON、CLI 参数或用户交互。
- 禁止 `cli` 在 `main.rs` 中堆叠具体检查逻辑或平台探测实现。
- 禁止把新的系统访问点分散回 `server.rs`、`main.rs`、`service.rs`。

## 当前状态
- 阶段 1 已完成：环境读取、命令查找、临时路径生成已集中到 `providers`/`config`。
- 阶段 2 已完成：`executor.rs` 已拆为 `service.rs` + `runtime.rs`。
- 阶段 3 已完成：`server.rs` 收紧为 HTTP 适配层，`main.rs` 收紧为 CLI 装配入口，`doctor` 独立到模块。
- 阶段 4 未完成：依赖边界尚未自动化检查，当前仍主要依赖代码结构和测试维持。
