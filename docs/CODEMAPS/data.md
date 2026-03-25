<!-- Generated: 2026-03-25 | Files scanned: 24 | Token estimate: ~650 -->
# 数据架构

## 持久化层
- 当前无数据库
- 当前无迁移文件
- 当前无 ORM / query builder

## 运行时数据模型
- `ExecuteRequest`
  - `language: String`
  - `code: String`
  - `timeout_ms: u64`
  - `memory_limit_mb: u32`
- `ExecuteResponse`
  - `stdout: String`
  - `stderr: String`
  - `exit_code: i32`
  - `duration_ms: u64`
- `SynapseError`
  - `InvalidInput`
  - `UnsupportedLanguage`
  - `Execution`
  - `Io`

## 内存态数据
- `SandboxPool`
  - `slots: VecDeque<PooledSandbox>`
  - `active`, `overflow_active`, `poisoned_total`
  - `requests_total`, `completed_total`, `failed_total`, `timeouts_total`
- `PreparedSandbox`
  - 仅保存临时工作目录路径
- `PoolMetrics`
  - 只读指标快照

## 文件/目录数据流
```
temp dir
  -> sandbox_dir()
  -> create_sandbox_dir()
  -> write main.py
  -> execute subprocess
  -> destroy/recreate after use
```

## 关系
```
SandboxPool
  1 -> many PooledSandbox
PooledSandbox
  1 -> 1 PreparedSandbox
PreparedSandbox
  1 -> 1 temp workspace path
```

## 关键文件
- `crates/synapse-core/src/types.rs`
- `crates/synapse-core/src/error.rs`
- `crates/synapse-core/src/pool.rs`
- `crates/synapse-core/src/executor.rs`
- `crates/synapse-api/src/server.rs`
