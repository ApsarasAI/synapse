# Synapse — 技术设计文档

> 本文档描述 Synapse 的系统架构、模块设计、数据流、关键算法和项目结构。
> 面向开发者，作为编码实现的直接参考。

---

## 当前实现校准

- 当前仓库只实现了 `synapse-core`、`synapse-api`、`synapse-cli` 三个 crate。
- 已实现的 HTTP 路由只有 `GET /health`、`GET /metrics`、`POST /execute`。
- 当前没有 `websocket.rs`、`middleware/`、`runtime/`、`cgroups/`、`filesystem/`、`synapse-enterprise`。
- 当前执行路径以 Python 为唯一语言；`RuntimeManager`、`Synapsefile`、多语言支持仍属于规划。
- Linux 执行必须依赖 `bwrap`；当前实现不再静默降级到 `unshare`。
- 下文凡涉及上述未来目录或类型，按目标架构理解，不代表当前代码实现。

## 目录

1. [系统架构总览](#1-系统架构总览)
2. [项目结构](#2-项目结构)
3. [核心模块设计](#3-核心模块设计)
4. [请求生命周期](#4-请求生命周期)
5. [沙箱预热池](#5-沙箱预热池)
6. [文件系统层](#6-文件系统层)
7. [安全隔离层](#7-安全隔离层)
8. [HTTP API 层](#8-http-api-层)
9. [运行时管理](#9-运行时管理)
10. [依赖与构建](#10-依赖与构建)
11. [关键数据结构](#11-关键数据结构)
12. [错误处理设计](#12-错误处理设计)
13. [配置体系](#13-配置体系)

---

## 1. 系统架构总览

### 1.1 分层架构

```
┌──────────────────────────────────────────────────────────┐
│                       客户端层                             │
│          HTTP Client / Python SDK / WebSocket             │
└────────────────────────┬─────────────────────────────────┘
                         │
┌────────────────────────▼─────────────────────────────────┐
│                  synapse-api (HTTP 服务层)                  │
│                                                           │
│  routes/        middleware/       websocket.rs   metrics  │
│  ├ execute.rs   ├ rate_limit.rs   (流式输出)     (/metrics)│
│  ├ health.rs    └ request_id.rs                           │
│  └ runtime.rs                                             │
└────────────────────────┬─────────────────────────────────┘
                         │ 调用
┌────────────────────────▼─────────────────────────────────┐
│                  synapse-core (核心引擎层)                   │
│                                                           │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ SandboxPool  │  │   Sandbox    │  │ RuntimeManager │  │
│  │  (预热池管理)  │  │  (沙箱实例)   │  │  (运行时管理)   │  │
│  └──────┬───────┘  └──────┬───────┘  └───────┬────────┘  │
│         │                 │                   │           │
│  ┌──────▼─────────────────▼───────────────────▼────────┐ │
│  │              Linux 内核抽象层                          │ │
│  │  ┌───────────┐ ┌─────────┐ ┌────────┐ ┌──────────┐ │ │
│  │  │ Namespace │ │ Seccomp │ │ Cgroups│ │ OverlayFS│ │ │
│  │  └───────────┘ └─────────┘ └────────┘ └──────────┘ │ │
│  └─────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
                         │
┌────────────────────────▼─────────────────────────────────┐
│                    Linux Kernel (>= 4.18)                  │
│     clone(2)  seccomp(2)  cgroup v2 fs  overlayfs         │
└──────────────────────────────────────────────────────────┘
```

### 1.2 Crate 依赖关系

```
synapse-cli ──────► synapse-api ──────► synapse-core
     │                   │
     │ (feature flag)    │
     ▼                   │
synapse-enterprise ──────┘

依赖方向：单向，Enterprise → Core，Core 永远不依赖 Enterprise
```

---

## 2. 项目结构

```
synapse/
├── Cargo.toml                      # workspace 根配置
├── Cargo.lock
├── LICENSE-APACHE                   # Core 许可证
├── LICENSE-ENTERPRISE               # Enterprise 许可证
├── README.md
├── SECURITY.md                      # 漏洞报告流程
│
├── crates/
│   ├── synapse-core/                # 核心引擎（开源）
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs               # 公共 API 导出
│   │       ├── sandbox/
│   │       │   ├── mod.rs           # Sandbox 结构体 + 生命周期
│   │       │   ├── pool.rs          # SandboxPool 预热池
│   │       │   └── executor.rs      # 代码执行逻辑
│   │       ├── namespace/
│   │       │   └── mod.rs           # Linux Namespace 封装
│   │       ├── seccomp/
│   │       │   ├── mod.rs           # Seccomp 过滤器
│   │       │   └── profiles.rs      # 预定义 Profile 模板
│   │       ├── cgroups/
│   │       │   └── mod.rs           # Cgroups v2 资源限制
│   │       ├── filesystem/
│   │       │   ├── mod.rs           # OverlayFS 挂载/卸载
│   │       │   └── rootfs.rs        # pivot_root + 文件系统设置
│   │       ├── runtime/
│   │       │   ├── mod.rs           # RuntimeManager
│   │       │   └── synapsefile.rs   # Synapsefile 解析
│   │       ├── config.rs            # 全局配置结构
│   │       └── error.rs             # 错误类型定义
│   │
│   ├── synapse-api/                 # HTTP API 层（开源）
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── server.rs            # axum Server 启动
│   │       ├── routes/
│   │       │   ├── mod.rs
│   │       │   ├── execute.rs       # POST /execute
│   │       │   ├── health.rs        # GET /health
│   │       │   └── metrics.rs       # GET /metrics
│   │       ├── websocket.rs         # WebSocket 流式输出
│   │       ├── middleware/
│   │       │   ├── mod.rs
│   │       │   └── rate_limit.rs    # 基础限流
│   │       └── types.rs             # 请求/响应类型定义
│   │
│   ├── synapse-enterprise/          # 企业功能（闭源）
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── audit/               # 审计日志
│   │       ├── multi_tenant/        # 多租户管理
│   │       ├── license/             # License Key 验证
│   │       └── sso/                 # SSO/LDAP 集成
│   │
│   └── synapse-cli/                 # CLI 入口（开源）
│       ├── Cargo.toml
│       └── src/
│           └── main.rs              # 命令行解析 + 启动
│
├── tests/
│   ├── integration/                 # 集成测试
│   │   ├── sandbox_lifecycle.rs
│   │   ├── namespace_isolation.rs
│   │   ├── filesystem_isolation.rs
│   │   └── api_endpoints.rs
│   ├── security/                    # 安全测试
│   │   ├── escape_tests.rs
│   │   └── seccomp_tests.rs
│   └── bench/                       # 性能基准
│       └── sandbox_bench.rs
│
├── fuzz/                            # 模糊测试
│   ├── Cargo.toml
│   └── fuzz_targets/
│       ├── fuzz_api_request.rs
│       └── fuzz_seccomp_profile.rs
│
├── sdk/
│   ├── python/                      # Python SDK
│   └── nodejs/                      # Node.js SDK
│
├── examples/                        # 集成示例
│   ├── fastapi_integration.py
│   └── langchain_tool.py
│
└── docs/
    ├── product/
    │   └── product.md               # 产品需求文档
    └── tech.md                      # 本文档
```

---

## 3. 核心模块设计

### 3.1 模块职责矩阵

| 模块 | Crate | 职责 | 对外接口 |
| :--- | :--- | :--- | :--- |
| `sandbox` | synapse-core | 沙箱实例的创建、执行、销毁 | `Sandbox::new()`, `Sandbox::execute()` |
| `sandbox::pool` | synapse-core | 预热池管理、获取/回收/扩缩容 | `SandboxPool::acquire()`, `SandboxPool::release()` |
| `namespace` | synapse-core | Linux Namespace 创建与配置 | `NamespaceConfig::apply()` |
| `seccomp` | synapse-core | Seccomp BPF 过滤器加载 | `SeccompProfile::load()`, `SeccompProfile::from_code()` |
| `cgroups` | synapse-core | Cgroups v2 资源限制 | `CgroupManager::setup()`, `CgroupManager::cleanup()` |
| `filesystem` | synapse-core | OverlayFS 挂载、pivot_root | `OverlayMount::mount()`, `setup_rootfs()` |
| `runtime` | synapse-core | 运行时镜像管理 | `RuntimeManager::get()`, `RuntimeManager::build()` |
| `routes` | synapse-api | HTTP 路由处理 | `POST /execute`, `GET /health`, `GET /metrics` |
| `websocket` | synapse-api | WebSocket 流式输出 | `WS /execute/stream` |

### 3.2 模块间调用关系

```
POST /execute 请求到达
       │
       ▼
  routes::execute
       │
       ├── 1. 参数校验 (types.rs)
       │
       ├── 2. 获取运行时 (RuntimeManager::get)
       │
       ├── 3. 获取沙箱 (SandboxPool::acquire)
       │       │
       │       ├── 池中有可用 → 直接返回
       │       └── 池为空 → Sandbox::new() 实时创建
       │               │
       │               ├── NamespaceConfig::apply()  → clone(2)
       │               ├── OverlayMount::mount()      → mount(2)
       │               ├── setup_rootfs()              → pivot_root(2)
       │               ├── CgroupManager::setup()      → 写 cgroup fs
       │               └── SeccompProfile::load()      → seccomp(2)
       │
       ├── 4. 执行代码 (Sandbox::execute)
       │       │
       │       ├── 写入用户代码到可写层
       │       ├── execvp() 启动语言解释器
       │       ├── 收集 stdout/stderr (pipe)
       │       └── 等待退出 或 超时 SIGKILL
       │
       ├── 5. 回收沙箱 (SandboxPool::release)
       │       │
       │       ├── 清理可写层
       │       ├── 重置环境变量
       │       └── 放回池中 或 标记 poisoned
       │
       └── 6. 返回响应
```

---

## 4. 请求生命周期

### 4.1 时序图

```
Client          API Server         SandboxPool        Sandbox           Linux Kernel
  │                 │                   │                │                   │
  │  POST /execute  │                   │                │                   │
  │────────────────►│                   │                │                   │
  │                 │                   │                │                   │
  │                 │  acquire()        │                │                   │
  │                 │──────────────────►│                │                   │
  │                 │                   │                │                   │
  │                 │  Sandbox (ready)  │                │                   │
  │                 │◄──────────────────│                │                   │
  │                 │                   │                │                   │
  │                 │  execute(code)    │                │                   │
  │                 │──────────────────────────────────►│                   │
  │                 │                   │                │                   │
  │                 │                   │                │  write code file  │
  │                 │                   │                │──────────────────►│
  │                 │                   │                │                   │
  │                 │                   │                │  execvp(python)   │
  │                 │                   │                │──────────────────►│
  │                 │                   │                │                   │
  │                 │                   │                │  stdout/stderr    │
  │                 │                   │                │◄──────────────────│
  │                 │                   │                │                   │
  │                 │  ExecuteResult    │                │                   │
  │                 │◄──────────────────────────────────│                   │
  │                 │                   │                │                   │
  │                 │  release(sandbox) │                │                   │
  │                 │──────────────────►│                │                   │
  │                 │                   │                │                   │
  │  JSON Response  │                   │  reset + pool  │                   │
  │◄────────────────│                   │                │                   │
  │                 │                   │                │                   │
```

### 4.2 延迟分解（预热池模式，目标 < 5ms）

| 阶段 | 操作 | 预期耗时 |
| :--- | :--- | :--- |
| T1 | HTTP 请求解析 + 参数校验 | ~0.1ms |
| T2 | 从预热池获取沙箱（lock + pop） | ~0.05ms |
| T3 | 写入用户代码到可写层 | ~0.2ms |
| T4 | execvp 启动 Python 解释器 | ~1-2ms（已预加载） |
| T5 | Python 执行用户代码 | 取决于代码 |
| T6 | 收集 stdout/stderr | ~0.1ms |
| T7 | 沙箱回收（清理可写层 + 放回池） | ~0.5ms |
| T8 | HTTP 响应序列化 | ~0.05ms |
| **总计（不含 T5）** | | **~2-3ms** |

---

## 5. 沙箱预热池

### 5.1 数据结构

```rust
pub struct SandboxPool {
    /// 可用沙箱队列
    available: Mutex<VecDeque<Sandbox>>,
    /// 池配置
    config: PoolConfig,
    /// 运行时指标
    metrics: PoolMetrics,
    /// 后台补充线程通知
    replenish_notify: Notify,
}

pub struct PoolConfig {
    /// 初始预热数量
    pub initial_size: usize,        // 默认 20
    /// 最大池容量
    pub max_size: usize,            // 默认 100
    /// 低水位阈值（触发补充）
    pub low_watermark: f64,         // 默认 0.2 (20%)
    /// 单次补充数量
    pub replenish_batch: usize,     // 默认 5
}

pub struct PoolMetrics {
    pub pool_size: AtomicUsize,
    pub pool_capacity: AtomicUsize,
    pub active_sandboxes: AtomicUsize,
    pub total_acquired: AtomicU64,
    pub total_recycled: AtomicU64,
    pub total_poisoned: AtomicU64,
    pub fallback_creates: AtomicU64,
}
```

### 5.2 核心算法

```
acquire():
    lock(available)
    if available.is_empty():
        metrics.fallback_creates += 1
        return Sandbox::new()           // 降级：实时创建
    sandbox = available.pop_front()
    metrics.pool_size -= 1
    metrics.active_sandboxes += 1
    unlock(available)

    if pool_size / max_size < low_watermark:
        replenish_notify.notify()       // 触发后台补充

    return sandbox

release(sandbox):
    if sandbox.is_poisoned():
        metrics.total_poisoned += 1
        spawn(sandbox.force_cleanup())  // 异步清理
        return

    sandbox.reset()                     // 清理可写层、重置环境
    lock(available)
    if available.len() < max_size:
        available.push_back(sandbox)
        metrics.pool_size += 1
    else:
        sandbox.destroy()               // 池满，直接销毁
    metrics.active_sandboxes -= 1
    unlock(available)

replenish_loop():                       // 后台 tokio task
    loop:
        replenish_notify.notified().await
        while pool_size < max_size * low_watermark:
            sandbox = Sandbox::new()
            lock(available)
            available.push_back(sandbox)
            metrics.pool_size += 1
            unlock(available)
```

### 5.3 沙箱状态机

```
                    ┌──────────┐
                    │ Creating │
                    └────┬─────┘
                         │ Namespace/FS/Seccomp 初始化完成
                         ▼
                    ┌──────────┐
         ┌─────────│  Ready   │◄────────────┐
         │         └────┬─────┘             │
         │              │ acquire()          │ release() + reset()
         │              ▼                    │
         │         ┌──────────┐             │
         │         │ Running  │─────────────┘
         │         └────┬─────┘
         │              │ 异常（无法回收）
         │              ▼
         │         ┌──────────┐
         │         │ Poisoned │
         │         └────┬─────┘
         │              │ 异步强制清理
         │              ▼
         └────────►┌──────────┐
                   │ Destroyed│
                   └──────────┘
```

---

## 6. 文件系统层

### 6.1 OverlayFS 架构

```
每个沙箱的文件系统视图：

mount -t overlay overlay \
  -o lowerdir=/var/synapse/layers/python-base,\
     upperdir=/var/synapse/sandboxes/<id>/upper,\
     workdir=/var/synapse/sandboxes/<id>/work \
  /var/synapse/sandboxes/<id>/merged

目录结构：
/var/synapse/
├── layers/                          # 只读层（共享）
│   ├── python-base/                 # Python 3.11 基础运行时
│   │   ├── usr/bin/python3
│   │   ├── usr/lib/python3.11/
│   │   └── ...
│   ├── python-scientific/           # Python + numpy/pandas
│   └── nodejs-base/                 # Node.js 20
│
└── sandboxes/                       # 沙箱实例（每个独立）
    └── <sandbox-id>/
        ├── upper/                   # 可写层（用户代码 + 临时文件）
        ├── work/                    # OverlayFS 工作目录
        └── merged/                  # 合并后的挂载点（沙箱根目录）
```

### 6.2 文件系统设置流程

```rust
pub fn setup_sandbox_fs(sandbox_id: &str, runtime: &str) -> Result<SandboxFs> {
    let base = PathBuf::from("/var/synapse/sandboxes").join(sandbox_id);

    // 1. 创建沙箱目录
    fs::create_dir_all(base.join("upper"))?;
    fs::create_dir_all(base.join("work"))?;
    fs::create_dir_all(base.join("merged"))?;

    // 2. 挂载 OverlayFS
    let lower = format!("/var/synapse/layers/{}", runtime);
    let opts = format!(
        "lowerdir={},upperdir={},workdir={}",
        lower,
        base.join("upper").display(),
        base.join("work").display()
    );
    mount(Some("overlay"), &base.join("merged"), Some("overlay"),
          MsFlags::empty(), Some(opts.as_str()))?;

    // 3. pivot_root 切换根目录
    let old_root = base.join("merged/old_root");
    fs::create_dir_all(&old_root)?;
    pivot_root(&base.join("merged"), &old_root)?;
    chdir("/")?;

    // 4. 挂载必要的虚拟文件系统
    mount_proc()?;          // /proc（独立 PID namespace 的 procfs）
    mount_dev_minimal()?;   // /dev（最小化 tmpfs：null, zero, urandom）
    mount_tmp()?;           // /tmp（tmpfs，用户代码写入位置）

    // 5. 卸载 old_root
    umount2("/old_root", MntFlags::MNT_DETACH)?;
    fs::remove_dir("/old_root")?;

    Ok(SandboxFs { base, sandbox_id: sandbox_id.to_string() })
}
```

### 6.3 沙箱重置（回收时）

```rust
impl SandboxFs {
    /// 重置可写层，恢复到干净状态
    pub fn reset(&self) -> Result<()> {
        // 卸载 overlay
        umount2(&self.base.join("merged"), MntFlags::MNT_DETACH)?;

        // 清空 upper 和 work 目录（不删除目录本身）
        remove_dir_contents(&self.base.join("upper"))?;
        remove_dir_contents(&self.base.join("work"))?;

        // 重新挂载 overlay
        self.remount_overlay()?;

        Ok(())
    }
}
```

---

## 7. 安全隔离层

### 7.1 Namespace 配置

```rust
pub struct NamespaceConfig {
    pub pid: bool,      // CLONE_NEWPID — 独立进程树
    pub mount: bool,    // CLONE_NEWNS  — 独立挂载点
    pub network: bool,  // CLONE_NEWNET — 独立网络栈
    pub ipc: bool,      // CLONE_NEWIPC — 独立 IPC
    pub uts: bool,      // CLONE_NEWUTS — 独立主机名
    pub user: bool,     // CLONE_NEWUSER — 用户映射（root → nobody）
}

impl NamespaceConfig {
    pub fn full_isolation() -> Self {
        Self {
            pid: true, mount: true, network: true,
            ipc: true, uts: true, user: true,
        }
    }

    pub fn to_clone_flags(&self) -> CloneFlags {
        let mut flags = CloneFlags::empty();
        if self.pid     { flags |= CloneFlags::CLONE_NEWPID; }
        if self.mount   { flags |= CloneFlags::CLONE_NEWNS; }
        if self.network { flags |= CloneFlags::CLONE_NEWNET; }
        if self.ipc     { flags |= CloneFlags::CLONE_NEWIPC; }
        if self.uts     { flags |= CloneFlags::CLONE_NEWUTS; }
        if self.user    { flags |= CloneFlags::CLONE_NEWUSER; }
        flags
    }
}
```

### 7.1.1 User Namespace UID/GID 映射

User Namespace 是纵深防御第 1 层的核心：确保沙箱内即使以 root 身份运行，在宿主机上也只是 `nobody`（UID 65534）。

```rust
/// 在子进程创建后、exec 之前调用
pub fn setup_user_namespace(child_pid: Pid) -> Result<()> {
    let pid = child_pid.as_raw();

    // 1. 写入 UID 映射：容器内 root (0) → 宿主机 nobody (65534)
    //    格式：<container_uid> <host_uid> <range>
    let uid_map = format!("0 65534 1\n");
    fs::write(format!("/proc/{}/uid_map", pid), &uid_map)
        .context("Failed to write uid_map")?;

    // 2. 禁用 setgroups（写入 uid_map 前的安全要求）
    fs::write(format!("/proc/{}/setgroups", pid), "deny\n")
        .context("Failed to deny setgroups")?;

    // 3. 写入 GID 映射：容器内 root (0) → 宿主机 nogroup (65534)
    let gid_map = format!("0 65534 1\n");
    fs::write(format!("/proc/{}/gid_map", pid), &gid_map)
        .context("Failed to write gid_map")?;

    tracing::info!(pid = pid, "User namespace UID/GID mapping configured: 0 → 65534");
    Ok(())
}
```

映射效果：

```
宿主机视角                          沙箱内视角
─────────────────                  ─────────────────
UID 65534 (nobody)    ←──映射──→   UID 0 (root)
GID 65534 (nogroup)   ←──映射──→   GID 0 (root)

沙箱内进程认为自己是 root，但在宿主机上实际权限为 nobody。
即使逃逸出 Namespace，也只有 nobody 的权限，无法读写系统文件。
```

调用时机（在 `Sandbox::new()` 中）：

```rust
impl Sandbox {
    pub fn new(config: &SandboxConfig) -> Result<Self> {
        // 1. clone() 创建子进程，带 CLONE_NEWUSER 等 flags
        let child_pid = unsafe {
            clone(child_entry, &mut stack, config.ns_config.to_clone_flags(), ...)?
        };

        // 2. 在父进程中配置子进程的 UID/GID 映射
        //    必须在子进程 exec 之前完成
        if config.ns_config.user {
            setup_user_namespace(Pid::from_raw(child_pid))?;
        }

        // 3. 通知子进程映射已就绪，可以继续初始化
        notify_child_ready()?;

        ...
    }
}
```

### 7.2 Seccomp Profile 设计

```rust
/// Seccomp 动作
pub enum SeccompAction {
    Allow,
    Kill,       // SECCOMP_RET_KILL_PROCESS
    Log,        // SECCOMP_RET_LOG（审计模式）
    Errno(i32), // SECCOMP_RET_ERRNO
}

/// 预定义 Profile 级别
pub enum ProfileLevel {
    /// 最严格：仅 read/write/exit/brk 等基础调用
    Minimal,
    /// 标准：Minimal + open/stat/fstat/mmap 等文件操作
    Standard,
    /// 科学计算：Standard + mprotect/mremap（numpy 需要）
    Scientific,
    /// 网络：Standard + socket/connect/sendto/recvfrom
    Network,
}

pub struct SeccompProfile {
    pub level: ProfileLevel,
    /// 白名单系统调用列表
    pub allowed_syscalls: Vec<i64>,
    /// 默认动作（白名单外的调用）
    pub default_action: SeccompAction,
}

impl SeccompProfile {
    /// 从 Profile 级别创建
    pub fn from_level(level: ProfileLevel) -> Self { ... }

    /// 从代码静态分析结果创建（Phase 2: 自适应）
    pub fn from_code_analysis(imports: &[String]) -> Self { ... }

    // TODO [Phase 2]: 自适应 Seccomp 设计
    //   - 静态分析器：解析 Python/Node.js import 语句，提取依赖列表
    //   - Profile 模板库：每个库对应的最小系统调用集合
    //   - --learn 模式：用 strace 跟踪执行，自动生成 + 缓存 profile
    //   - 详见 product.md 5.3 节

    /// 加载到当前进程
    pub fn load(&self) -> Result<()> {
        let mut ctx = ScmpFilterCtx::new(self.default_action.to_scmp())?;
        for &syscall in &self.allowed_syscalls {
            ctx.add_rule(ScmpAction::Allow, syscall)?;
        }
        ctx.load()?;
        Ok(())
    }
}
```

### 7.3 Seccomp 白名单参考（Minimal Profile）

```
# 基础 I/O
read, write, readv, writev, close

# 内存管理
brk, mmap, munmap

# 进程控制
exit, exit_group, rt_sigreturn

# 文件描述符
dup, dup2, fcntl, ioctl

# 信息查询
getpid, getuid, getgid, gettimeofday, clock_gettime

# Python 解释器必需
access, fstat, stat, lstat, lseek, openat
getcwd, readlink, getrandom
futex, set_robust_list, set_tid_address
arch_prctl, prlimit64, sched_getaffinity
```

### 7.3.1 安全网络模式

> **TODO [Phase 2]:** 当前 Network Namespace 仅支持全开（`network: false`）或全关（`network: true`）。
> 需要设计"安全网络模式"：在 Network Namespace 内创建 veth pair，通过 iptables/nftables 规则仅允许特定域名或 IP 的出站连接。
> 详见 product.md F3 验收标准。

### 7.4 Cgroups v2 管理

```rust
pub struct CgroupManager {
    path: PathBuf,  // /sys/fs/cgroup/synapse/<sandbox-id>
}

impl CgroupManager {
    pub fn new(sandbox_id: &str) -> Result<Self> {
        let path = PathBuf::from("/sys/fs/cgroup/synapse").join(sandbox_id);
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub fn setup(&self, limits: &ResourceLimits) -> Result<()> {
        // 内存限制
        if let Some(memory_mb) = limits.memory_limit_mb {
            let bytes = (memory_mb as u64) * 1024 * 1024;
            fs::write(self.path.join("memory.max"), bytes.to_string())?;
            // 禁用 swap
            fs::write(self.path.join("memory.swap.max"), "0")?;
        }

        // CPU 限制（转换为 cpu.max 格式：quota period）
        if let Some(timeout_ms) = limits.timeout_ms {
            let quota = timeout_ms * 1000; // 微秒
            fs::write(self.path.join("cpu.max"), format!("{} 100000", quota))?;
        }

        // PID 数量限制（防止 fork bomb）
        fs::write(self.path.join("pids.max"), limits.max_pids.to_string())?;

        Ok(())
    }

    pub fn add_process(&self, pid: u32) -> Result<()> {
        fs::write(self.path.join("cgroup.procs"), pid.to_string())?;
        Ok(())
    }

    pub fn cleanup(&self) -> Result<()> {
        // 先杀掉 cgroup 内所有进程
        fs::write(self.path.join("cgroup.kill"), "1").ok();
        // 等待进程退出后删除目录
        fs::remove_dir(&self.path).ok();
        Ok(())
    }
}
```

---

## 8. HTTP API 层

### 8.0 WebSocket 流式输出设计

支持通过 WebSocket 长连接实时接收沙箱 stdout/stderr 输出，适合交互式 AI 对话场景。

#### 连接流程

```
Client                              API Server                    Sandbox
  │                                     │                            │
  │  GET /execute/stream (Upgrade)      │                            │
  │────────────────────────────────────►│                            │
  │                                     │                            │
  │  101 Switching Protocols            │                            │
  │◄────────────────────────────────────│                            │
  │                                     │                            │
  │  WS Text: ExecuteRequest JSON       │                            │
  │────────────────────────────────────►│                            │
  │                                     │  acquire + execute         │
  │                                     │───────────────────────────►│
  │                                     │                            │
  │  WS Text: {"type":"stdout",...}     │  stdout pipe (非阻塞读取)   │
  │◄────────────────────────────────────│◄───────────────────────────│
  │  WS Text: {"type":"stdout",...}     │                            │
  │◄────────────────────────────────────│◄───────────────────────────│
  │  WS Text: {"type":"stderr",...}     │                            │
  │◄────────────────────────────────────│◄───────────────────────────│
  │                                     │                            │
  │                                     │  进程退出                   │
  │                                     │◄───────────────────────────│
  │  WS Text: {"type":"done",...}       │                            │
  │◄────────────────────────────────────│                            │
  │                                     │  release sandbox           │
  │  WS Close                           │                            │
  │◄───────────────────────────────────►│                            │
```

#### 消息协议

客户端发送（仅一次，连接建立后）：
```json
{
  "language": "python",
  "code": "for i in range(5): print(i)",
  "timeout_ms": 5000,
  "memory_limit_mb": 128
}
```

服务端推送（多次）：
```json
{"type": "stdout", "data": "0\n", "timestamp": "2024-07-15T10:30:00.012Z"}
{"type": "stdout", "data": "1\n", "timestamp": "2024-07-15T10:30:00.013Z"}
{"type": "stderr", "data": "Warning: ...\n", "timestamp": "2024-07-15T10:30:00.014Z"}
{"type": "done", "exit_code": 0, "duration_ms": 15}
```

错误情况：
```json
{"type": "error", "error_type": "TIMEOUT", "message": "Execution exceeded 5000ms limit"}
```

#### 实现设计

```rust
pub async fn handle(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_stream(socket, state))
}

async fn handle_stream(mut socket: WebSocket, state: AppState) {
    // 1. 接收执行请求（第一条消息）
    let req: ExecuteRequest = match socket.recv().await {
        Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
            Ok(req) => req,
            Err(e) => {
                send_error(&mut socket, "INVALID_INPUT", &e.to_string()).await;
                return;
            }
        },
        _ => return,
    };

    // 2. 获取运行时 + 沙箱
    let runtime = match state.runtime_manager.get(&req.language) {
        Some(r) => r,
        None => {
            send_error(&mut socket, "UNSUPPORTED_LANGUAGE", &req.language).await;
            return;
        }
    };
    let sandbox = match state.pool.acquire().await {
        Ok(sb) => sb,
        Err(_) => {
            send_error(&mut socket, "SERVICE_UNAVAILABLE", "pool exhausted").await;
            return;
        }
    };

    // 3. 启动执行，通过 channel 接收流式输出
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);

    let exec_handle = tokio::spawn(async move {
        sandbox.execute_streaming(ExecuteParams {
            code: req.code,
            runtime,
            timeout_ms: req.timeout_ms,
            memory_limit_mb: req.memory_limit_mb,
        }, tx).await
    });

    // 4. 转发 channel 事件到 WebSocket
    while let Some(event) = rx.recv().await {
        let msg = serde_json::to_string(&event).unwrap();
        if socket.send(Message::Text(msg)).await.is_err() {
            break; // 客户端断开
        }
    }

    // 5. 等待执行完成，回收沙箱
    if let Ok(sandbox) = exec_handle.await {
        state.pool.release(sandbox).await;
    }
}

/// 流式输出事件
#[derive(Serialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "stdout")]
    Stdout { data: String, timestamp: String },
    #[serde(rename = "stderr")]
    Stderr { data: String, timestamp: String },
    #[serde(rename = "done")]
    Done { exit_code: i32, duration_ms: u64 },
    #[serde(rename = "error")]
    Error { error_type: String, message: String },
}
```

#### 沙箱侧流式输出收集

```rust
impl Sandbox {
    /// 流式执行：通过 channel 逐行推送 stdout/stderr
    pub async fn execute_streaming(
        &self,
        params: ExecuteParams,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<()> {
        let child = self.spawn_child(&params)?;

        // 非阻塞读取 stdout/stderr pipe
        let stdout_pipe = child.stdout.take().unwrap();
        let stderr_pipe = child.stderr.take().unwrap();

        let tx_out = tx.clone();
        let stdout_task = tokio::spawn(async move {
            let reader = BufReader::new(stdout_pipe);
            let mut lines = reader.lines();
            while let Some(line) = lines.next_line().await? {
                let _ = tx_out.send(StreamEvent::Stdout {
                    data: format!("{}\n", line),
                    timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                }).await;
            }
            Ok::<(), anyhow::Error>(())
        });

        let tx_err = tx.clone();
        let stderr_task = tokio::spawn(async move {
            let reader = BufReader::new(stderr_pipe);
            let mut lines = reader.lines();
            while let Some(line) = lines.next_line().await? {
                let _ = tx_err.send(StreamEvent::Stderr {
                    data: format!("{}\n", line),
                    timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                }).await;
            }
            Ok::<(), anyhow::Error>(())
        });

        // 等待进程退出（带超时）
        let start = Instant::now();
        let status = tokio::time::timeout(
            Duration::from_millis(params.timeout_ms),
            child.wait()
        ).await;

        let _ = stdout_task.await;
        let _ = stderr_task.await;

        match status {
            Ok(Ok(exit)) => {
                let _ = tx.send(StreamEvent::Done {
                    exit_code: exit.code().unwrap_or(-1),
                    duration_ms: start.elapsed().as_millis() as u64,
                }).await;
            }
            Ok(Err(e)) => {
                let _ = tx.send(StreamEvent::Error {
                    error_type: "INTERNAL_ERROR".to_string(),
                    message: e.to_string(),
                }).await;
            }
            Err(_) => {
                child.kill().await.ok();
                let _ = tx.send(StreamEvent::Error {
                    error_type: "TIMEOUT".to_string(),
                    message: format!("Execution exceeded {}ms limit", params.timeout_ms),
                }).await;
            }
        }

        Ok(())
    }
}
```

### 8.1 技术选型

| 组件 | 选择 | 理由 |
| :--- | :--- | :--- |
| HTTP 框架 | axum | Rust 生态最活跃、基于 tokio、类型安全路由 |
| 序列化 | serde + serde_json | Rust 标准选择 |
| Metrics | prometheus-client | 轻量、无全局状态 |
| WebSocket | axum 内置 | 无需额外依赖 |
| 日志 | tracing + tracing-subscriber | 结构化日志、span 追踪 |

### 8.2 路由定义

```rust
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // 核心接口
        .route("/execute", post(routes::execute::handle))
        .route("/execute/stream", get(websocket::handle))
        // 运维接口
        .route("/health", get(routes::health::handle))
        .route("/metrics", get(routes::metrics::handle))
        // 运行时管理
        .route("/runtimes", get(routes::runtime::list))
        // 中间件
        .layer(middleware::rate_limit::layer())
        .layer(middleware::request_id::layer())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub struct AppState {
    pub pool: Arc<SandboxPool>,
    pub runtime_manager: Arc<RuntimeManager>,
    pub metrics: Arc<AppMetrics>,
    pub config: Arc<SynapseConfig>,
}
```

### 8.3 执行接口详细设计

```rust
// 请求类型
#[derive(Deserialize, Validate)]
pub struct ExecuteRequest {
    /// 编程语言
    pub language: String,
    /// 用户代码
    #[validate(length(min = 1, max = 1_048_576))]  // 最大 1MB
    pub code: String,
    /// 超时时间（毫秒），默认 5000，上限 30000
    #[serde(default = "default_timeout")]
    #[validate(range(min = 100, max = 30_000))]
    pub timeout_ms: u64,
    /// 内存限制（MB），默认 128，上限 256
    #[serde(default = "default_memory")]
    #[validate(range(min = 16, max = 256))]
    pub memory_limit_mb: u32,
}

// 响应类型
#[derive(Serialize)]
pub struct ExecuteResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ExecuteError>,
}

#[derive(Serialize)]
pub struct ExecuteError {
    pub r#type: String,     // TIMEOUT, OOM_KILLED, SYSCALL_BLOCKED, ...
    pub message: String,
    pub detail: String,
}
```

### 8.4 执行处理流程

```rust
pub async fn handle(
    State(state): State<AppState>,
    Json(req): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResponse>, ApiError> {
    // 1. 参数校验
    req.validate()?;

    // 2. 获取运行时
    let runtime = state.runtime_manager
        .get(&req.language)
        .ok_or(ApiError::UnsupportedLanguage(req.language.clone()))?;

    // 3. 从池中获取沙箱
    let sandbox = state.pool.acquire().await
        .map_err(|_| ApiError::ServiceUnavailable)?;

    // 4. 执行代码（带超时）
    let result = tokio::time::timeout(
        Duration::from_millis(req.timeout_ms + 1000), // 额外 1s 清理时间
        sandbox.execute(ExecuteParams {
            code: req.code,
            runtime: runtime.clone(),
            timeout_ms: req.timeout_ms,
            memory_limit_mb: req.memory_limit_mb,
        })
    ).await;

    // 5. 回收沙箱
    state.pool.release(sandbox).await;

    // 6. 构造响应
    match result {
        Ok(Ok(output)) => Ok(Json(output.into())),
        Ok(Err(e)) => Ok(Json(e.into_response())),
        Err(_) => Ok(Json(ExecuteResponse::timeout(req.timeout_ms))),
    }
}
```

---

## 9. 运行时管理

### 9.1 RuntimeManager

```rust
pub struct RuntimeManager {
    /// 已注册的运行时 <name, Runtime>
    runtimes: RwLock<HashMap<String, Runtime>>,
    /// 只读层存储路径
    layers_dir: PathBuf,
}

pub struct Runtime {
    pub name: String,           // "python-base"
    pub version: String,        // "3.11.9"
    pub language: String,       // "python"
    pub layer_path: PathBuf,    // "/var/synapse/layers/python-base"
    pub interpreter: String,    // "/usr/bin/python3"
    pub interpreter_args: Vec<String>,  // ["-c"]（用于内联代码执行）
    pub size_bytes: u64,
}

impl RuntimeManager {
    /// 根据语言名获取运行时
    pub fn get(&self, language: &str) -> Option<Runtime> {
        self.runtimes.read().values()
            .find(|r| r.language == language)
            .cloned()
    }

    /// 从 Synapsefile 构建运行时
    pub fn build(&self, synapsefile: &Synapsefile) -> Result<Runtime> {
        // 1. 拉取 base image（或从本地缓存）
        // 2. 在临时容器中执行 install 命令
        // 3. 打包为只读层
        // 4. 注册到 runtimes map
        ...
    }

    /// 扫描 layers_dir，加载已有运行时
    pub fn scan_and_load(&self) -> Result<()> { ... }
}
```

### 9.2 代码执行方式

```rust
impl Sandbox {
    pub fn execute(&self, params: ExecuteParams) -> Result<ExecuteOutput> {
        // 1. 将用户代码写入可写层
        let code_path = "/tmp/user_code.py";  // 沙箱内路径
        fs::write(
            self.merged_path().join("tmp/user_code.py"),
            &params.code
        )?;

        // 2. 构造执行命令
        //    python3 /tmp/user_code.py
        //    或 python3 -c "code..." （短代码）
        let cmd = if params.code.len() < 4096 {
            vec![
                params.runtime.interpreter.clone(),
                "-c".to_string(),
                params.code.clone(),
            ]
        } else {
            vec![
                params.runtime.interpreter.clone(),
                code_path.to_string(),
            ]
        };

        // 3. 在沙箱子进程中 execvp
        let child = self.spawn_child(&cmd)?;

        // 4. 收集输出（带超时）
        let output = self.collect_output(child, params.timeout_ms)?;

        Ok(output)
    }
}
```

---

## 10. 依赖与构建

### 10.1 Workspace Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "crates/synapse-core",
    "crates/synapse-api",
    "crates/synapse-cli",
    # "crates/synapse-enterprise",  # 闭源，按需启用
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
repository = "https://github.com/ApsarasAI/synapse"

[workspace.dependencies]
# 异步运行时
tokio = { version = "1", features = ["full"] }
# HTTP
axum = "0.7"
tower = "0.4"
tower-http = { version = "0.5", features = ["trace", "cors"] }
# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"
# Linux 系统调用
nix = { version = "0.29", features = ["sched", "mount", "signal", "hostname", "fs", "process"] }
# 错误处理
anyhow = "1"
thiserror = "1"
# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
# 指标
prometheus-client = "0.22"
# 工具
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
validator = { version = "0.18", features = ["derive"] }
```

### 10.2 各 Crate 依赖

```toml
# crates/synapse-core/Cargo.toml
[dependencies]
nix = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true }
# Seccomp
libseccomp = "0.3"

# crates/synapse-api/Cargo.toml
[dependencies]
synapse-core = { path = "../synapse-core" }
axum = { workspace = true }
tokio = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
prometheus-client = { workspace = true }
validator = { workspace = true }

# crates/synapse-cli/Cargo.toml
[dependencies]
synapse-core = { path = "../synapse-core" }
synapse-api = { path = "../synapse-api" }
clap = { version = "4", features = ["derive"] }
tokio = { workspace = true }
tracing-subscriber = { workspace = true }

[features]
default = []
enterprise = ["synapse-enterprise"]

[dependencies.synapse-enterprise]
path = "../synapse-enterprise"
optional = true
```

### 10.3 构建命令

```bash
# 开发构建
cargo build

# Release 构建（Core 版本）
cargo build --release --bin synapse

# Release 构建（Enterprise 版本）
cargo build --release --bin synapse --features enterprise

# 静态链接（musl）— 用于分发
cargo build --release --target x86_64-unknown-linux-musl --bin synapse
cargo build --release --target aarch64-unknown-linux-musl --bin synapse

# 运行测试
cargo test --workspace
cargo test --test integration -- --test-threads=1  # 集成测试需串行

# 运行 benchmark
cargo bench --bench sandbox_bench
```

---

## 11. 关键数据结构

### 11.1 沙箱实例

```rust
pub struct Sandbox {
    /// 唯一标识
    pub id: String,
    /// 当前状态
    pub state: SandboxState,
    /// 子进程 PID（Running 状态下有值）
    pub child_pid: Option<Pid>,
    /// 文件系统句柄
    pub fs: SandboxFs,
    /// Cgroup 管理器
    pub cgroup: CgroupManager,
    /// Namespace 配置
    pub ns_config: NamespaceConfig,
    /// Seccomp Profile
    pub seccomp_profile: SeccompProfile,
    /// 创建时间
    pub created_at: Instant,
    /// 使用次数（用于判断是否需要重建）
    pub use_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SandboxState {
    Creating,
    Ready,
    Running,
    Poisoned { reason: String },
    Destroyed,
}
```

### 11.2 执行参数与输出

```rust
pub struct ExecuteParams {
    pub code: String,
    pub runtime: Runtime,
    pub timeout_ms: u64,
    pub memory_limit_mb: u32,
}

pub struct ExecuteOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub memory_peak_bytes: u64,
    pub error: Option<SandboxError>,
}
```

### 11.3 资源限制

```rust
pub struct ResourceLimits {
    /// 最大内存（MB）
    pub memory_limit_mb: Option<u32>,
    /// 执行超时（毫秒）
    pub timeout_ms: Option<u64>,
    /// 最大 PID 数量（防 fork bomb）
    pub max_pids: u32,           // 默认 10
    /// 最大输出大小（字节）
    pub max_output_bytes: usize, // 默认 1MB
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory_limit_mb: Some(128),
            timeout_ms: Some(5000),
            max_pids: 10,
            max_output_bytes: 1_048_576,
        }
    }
}
```

### 11.4 全局配置

```rust
pub struct SynapseConfig {
    /// 服务监听地址
    pub listen_addr: SocketAddr,        // 默认 0.0.0.0:8080
    /// 沙箱池配置
    pub pool: PoolConfig,
    /// 默认资源限制
    pub default_limits: ResourceLimits,
    /// 运行时层存储路径
    pub layers_dir: PathBuf,            // 默认 /var/synapse/layers
    /// 沙箱工作目录
    pub sandboxes_dir: PathBuf,         // 默认 /var/synapse/sandboxes
    /// 日志级别
    pub log_level: String,              // 默认 "info"
    /// Metrics 是否启用
    pub metrics_enabled: bool,          // 默认 true
}
```

---

## 12. 错误处理设计

### 12.1 错误类型层次

```rust
// synapse-core 错误
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("Execution timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("Process killed by OOM (memory limit: {limit_mb}MB)")]
    OomKilled { limit_mb: u32 },

    #[error("Syscall blocked by seccomp: {syscall}")]
    SyscallBlocked { syscall: String },

    #[error("Sandbox is poisoned: {reason}")]
    Poisoned { reason: String },

    #[error("Output truncated at {max_bytes} bytes")]
    OutputTruncated { max_bytes: usize },

    #[error("Namespace setup failed: {0}")]
    NamespaceError(#[source] nix::Error),

    #[error("Filesystem setup failed: {0}")]
    FilesystemError(#[source] std::io::Error),

    #[error("Cgroup setup failed: {0}")]
    CgroupError(#[source] std::io::Error),

    #[error("Seccomp load failed: {0}")]
    SeccompError(String),
}

// synapse-api 错误（面向用户）
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("Service unavailable: sandbox pool exhausted")]
    ServiceUnavailable,

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type) = match &self {
            ApiError::InvalidInput(_) => (StatusCode::BAD_REQUEST, "INVALID_INPUT"),
            ApiError::UnsupportedLanguage(_) => (StatusCode::BAD_REQUEST, "UNSUPPORTED_LANGUAGE"),
            ApiError::ServiceUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let body = json!({
            "error": {
                "type": error_type,
                "message": self.to_string(),
            }
        });

        (status, Json(body)).into_response()
    }
}
```

### 12.2 SandboxError → ExecuteResponse 映射

```rust
impl From<SandboxError> for ExecuteResponse {
    fn from(err: SandboxError) -> Self {
        let (error_type, message) = match &err {
            SandboxError::Timeout { .. }        => ("TIMEOUT", err.to_string()),
            SandboxError::OomKilled { .. }      => ("OOM_KILLED", err.to_string()),
            SandboxError::SyscallBlocked { .. } => ("SYSCALL_BLOCKED", err.to_string()),
            SandboxError::OutputTruncated { .. } => ("OUTPUT_TRUNCATED", err.to_string()),
            _ => ("INTERNAL_ERROR", "Sandbox execution failed".to_string()),
        };

        ExecuteResponse {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: -1,
            duration_ms: 0,
            error: Some(ExecuteError {
                r#type: error_type.to_string(),
                message,
                detail: format!("{:?}", err),
            }),
        }
    }
}
```

---

## 13. 配置体系

### 13.1 配置文件格式

```toml
# /etc/synapse/config.toml（或 ./synapse.toml）

[server]
listen_addr = "0.0.0.0:8080"
log_level = "info"
metrics_enabled = true

[pool]
initial_size = 20
max_size = 100
low_watermark = 0.2
replenish_batch = 5

[limits]
default_timeout_ms = 5000
default_memory_limit_mb = 128
max_timeout_ms = 30000
max_memory_limit_mb = 256
max_pids = 10
max_output_bytes = 1048576

[storage]
layers_dir = "/var/synapse/layers"
sandboxes_dir = "/var/synapse/sandboxes"

[seccomp]
# minimal | standard | scientific | network
default_profile = "standard"
# 是否启用自适应 profile（Phase 2）
adaptive_enabled = false
```

### 13.2 配置优先级

```
命令行参数 > 环境变量 > 配置文件 > 默认值

环境变量命名规则：SYNAPSE_ 前缀 + 大写 + 下划线
  例：SYNAPSE_SERVER_LISTEN_ADDR=0.0.0.0:9090
      SYNAPSE_POOL_INITIAL_SIZE=50
```

### 13.3 CLI 入口

```rust
// crates/synapse-cli/src/main.rs

#[derive(Parser)]
#[command(name = "synapse", about = "AI code execution sandbox")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Synapse server
    Serve {
        /// Config file path
        #[arg(short, long, default_value = "/etc/synapse/config.toml")]
        config: PathBuf,
        /// Listen address (overrides config)
        #[arg(long)]
        listen: Option<SocketAddr>,
    },
    /// Manage language runtimes
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },
    /// Check system requirements
    Doctor,
}

#[derive(Subcommand)]
enum RuntimeAction {
    /// List installed runtimes
    List,
    /// Build a runtime from Synapsefile
    Build {
        #[arg(short, long, default_value = "Synapsefile")]
        file: PathBuf,
    },
    /// Remove a runtime
    Remove { name: String },
    /// Export a runtime for offline transfer
    Export { name: String, output: PathBuf },
    /// Import a runtime from file
    Import { input: PathBuf },
}
```

### 13.4 synapse doctor 输出

```
$ synapse doctor

Synapse System Check
====================

[✓] Linux kernel version: 5.15.0 (>= 4.18 required)
[✓] Cgroups v2: enabled, mounted at /sys/fs/cgroup
[✓] Namespaces: PID ✓  Mount ✓  Network ✓  IPC ✓  UTS ✓  User ✓
[✓] OverlayFS: supported
[✓] Seccomp: supported (libseccomp 2.5.4)
[✓] Storage: /var/synapse (12GB available)
[✓] Python runtime: python-base 3.11.9 (85MB)

All checks passed. Synapse is ready to run.
```

---

## 14. 待补充设计（TODO）

以下内容在 `../product/product.md` 中已定义需求，本文尚未展开技术设计，按 Phase 排期逐步补充。

| 编号 | 功能 | 对应 product.md | 排期 | 说明 |
| :--- | :--- | :--- | :--- | :--- |
| TODO-1 | 审计日志模块 | F6 + 第 14 章 | Phase 3 / Enterprise | 需在 synapse-core Sandbox 生命周期中预留审计 hook 点（on_file_access / on_network_connect / on_exec），synapse-enterprise 实现 AuditPlugin 订阅事件 |
| TODO-2 | io_uring 异步调度 | 5.4 节 | Phase 3 | 替换当前 tokio spawn 模型，用 io_uring 管理沙箱进程 IO 收集和超时控制 |
| TODO-3 | WASM 双引擎 | 5.5 节 | Phase 4 | 集成 Wasmtime，实现依赖分析 → 引擎路由逻辑 |
| TODO-4 | API 层 Metrics 补全 | 第 9 章可观测性 | Phase 1 | 当前仅有 PoolMetrics，需补充 `request_duration_ms` (Histogram)、`request_total` (Counter) 等 API 层指标 |
| TODO-5 | 多租户配额管理 | 第 8 章 | Phase 2 / Enterprise | 租户识别（API Key）、并发配额、RPM 限流、优先级调度 |
