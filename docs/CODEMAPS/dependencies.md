<!-- Generated: 2026-03-25 | Files scanned: 24 | Token estimate: ~750 -->
# 依赖架构

## 外部运行时 / 系统依赖
- `python3`：当前唯一支持的执行语言
- `bwrap`：Linux 下唯一支持的沙箱策略
- `libseccomp`：seccomp 黑名单导出与加载
- Linux kernel：`setrlimit`, `prctl`, `seccomp`, namespace 相关能力
- 临时目录文件系统：代码执行工作区

## 第三方 Rust 依赖
- `axum`：HTTP API
- `tokio`：异步运行时、进程管理、测试
- `serde` / `serde_json`：请求和响应序列化
- `clap`：CLI 参数解析
- `thiserror`：错误类型
- `libc`：底层系统调用封装
- `tower`：测试中使用的 `ServiceExt`

## 工作区内部依赖
```
synapse-cli -> synapse-api -> synapse-core
synapse-api  -> synapse-core
```

## 共享库 / 模块
- `synapse-core::types`
- `synapse-core::error`
- `synapse-core::executor`
- `synapse-core::pool`
- `synapse-core::seccomp`

## 未使用的潜在依赖
- 当前没有数据库
- 当前没有 Redis
- 当前没有外部支付、消息队列、对象存储或前端 SDK 依赖
