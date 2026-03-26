
# Synapse — 产品需求文档 (PRD)

> 专为 AI Agent 设计的、毫秒级启动的轻量级代码执行沙箱。

| 字段 | 内容 |
| :--- | :--- |
| 项目名称 | Synapse（寓意极小、极快） |
| 当前版本 | v0.2.0 (Phase 1 完成，进入 Phase 2) |
| 技术栈 | Rust / Linux Kernel Features |
| 更新日期 | 2026 年 3 月 |

名称含义：神经元之间传递信号的连接点。
名称解读：大模型是大脑，代码执行是动作，而你的沙箱就是那个传递指令的"突触"。没有它，大脑有想法也动不了。
Slogan：Connect Thought to Action.（连接思想与行动）

---

## 目录

1. [产品愿景](#1-产品愿景)
2. [目标用户](#2-目标用户)
3. [竞品分析](#3-竞品分析)
4. [功能需求](#4-功能需求)
5. [技术创新方案](#5-技术创新方案)
6. [安全架构与威胁模型](#6-安全架构与威胁模型)
7. [错误处理与边界场景](#7-错误处理与边界场景)
8. [多租户与并发模型](#8-多租户与并发模型)
9. [可观测性](#9-可观测性)
10. [SDK 与集成方案](#10-sdk-与集成方案)
11. [语言运行时管理](#11-语言运行时管理)
12. [非功能性需求](#12-非功能性需求)
13. [商业模式与定价策略](#13-商业模式与定价策略)
14. [开源策略](#14-开源策略)
15. [测试策略](#15-测试策略)
16. [开发路线图](#16-开发路线图)

---

## 1. 产品愿景

### 要解决的问题

现有容器技术（Docker、MicroVM）在 AI 代码执行场景下存在三大痛点：

| 痛点 | 表现 |
| :--- | :--- |
| 启动慢 | Docker 冷启动 1-5 秒，严重影响 AI 对话体验 |
| 资源重 | MicroVM 方案内存开销大，单机并发受限 |
| 配置复杂 | 安全隔离需要大量手动配置，私有化部署门槛高 |

### 我们的答案

利用 Rust + Linux 内核特性，提供：

- **< 10ms 冷启动** — 进程级隔离，跳过虚拟机开销
- **原生级安全** — Namespace + Seccomp + Cgroups 三层防护
- **零依赖部署** — 单二进制文件，无需 Docker/K8s

支持公有云 API 与私有化部署两种模式。

### 产品现状（2026-03）

**已完成核心能力：**

| 能力 | 状态 | 说明 |
|-----|------|-----|
| 沙箱执行引擎 | ✅ 生产就绪 | Namespace/Seccomp/Cgroups 三层隔离 |
| OverlayFS 文件系统 | ✅ 生产就绪 | 共享只读层 + 独立写层，支持池化重置 |
| 审计日志 | ✅ 生产就绪 | syscall 级细粒度审计，支持事件检索 |
| 多租户调度 | ✅ 生产就绪 | 配额、公平队列、背压机制 |
| 资源限制 | ✅ 生产就绪 | 内存/CPU/wall timeout 独立控制 |
| Runtime 管理 | ⚠️ MVP 可用 | CLI 支持显式 install-bundle/import/verify，仅 Python，host import 仍依赖宿主解释器 |

**待完善能力：**

| 能力 | 状态 | 优先级 |
|-----|------|-------|
| 多语言支持 | 🔴 待开发 | P2 |
| Python SDK | 🔴 待开发 | P1 |
| 性能验证 | ⚠️ 缺数据 | P1 |
| 白名单 Seccomp | 🔴 待开发 | P2 |

---

## 2. 目标用户

### 2.1 核心用户：AI 应用开发者 / Agent 架构师

- **场景：** 开发类似 ChatGPT Code Interpreter 的功能
- **痛点：** Docker 冷启动太慢影响对话体验；担心 AI 生成的恶意代码通过 `os.system` 逃逸
- **期望：** 一个 HTTP API，发一段 Python 代码，极快返回结果，宿主机绝对安全

### 2.2 次级用户：企业安全团队（金融 / 医疗）

- **场景：** 数据合规要求极高，数据不能出域
- **痛点：** 无法使用公有云 SaaS（如 E2B），内网部署 K8s 成本过高
- **期望：** 一个可下载的单二进制文件，在物理隔离环境中直接运行

---

## 3. 竞品分析

### 3.1 横向对比

| 维度 | Docker / runC | E2B | Modal | **Synapse** |
| :--- | :--- | :--- | :--- | :--- |
| 启动速度 | 秒级 (1-5s) | 毫秒级 | 毫秒级 | **< 10ms（目标 1-3ms）** |
| 隔离技术 | Namespace/Cgroups | MicroVM (Firecracker) | MicroVM | **进程级隔离 + Seccomp** |
| 资源开销 | 高 (MB~GB) | 中等 | 中等 | **极低（仅进程开销）** |
| 部署难度 | 中（需守护进程） | 高（依赖云/K8s） | 高（依赖云） | **极低（单二进制文件）** |
| 私有化部署 | 支持 | 有限 | 不支持 | **原生支持** |
| 审计能力 | 基础 | 基础 | 基础 | **细粒度审计日志** |

### 3.2 差异化切入点

1. **离线/私有化** — "Drop-in Binary"，解耦云依赖，解决企业数据不出域的刚需
2. **审计与合规** — 细粒度操作日志（文件读写、网络连接、执行命令），满足金融级风控
3. **极致轻量** — 无守护进程、无虚拟机，资源利用率远超竞品

---

## 4. 功能需求

### 4.1 核心引擎（MVP 必备）

#### F1 — 极速隔离环境

使用 Rust 调用 Linux `clone` 系统调用创建子进程，应用 Namespace 隔离（PID / Mount / Network / IPC / UTS）。

验收标准：
- 执行 `print("hello")`，端到端延迟（含启动）< 20ms
- 子进程内无法访问父进程的 PID 和 IPC 资源

#### F2 — 文件系统隔离

使用 `pivot_root` 将沙箱根目录切换到临时目录，配合 OverlayFS 分层架构（详见 [5.2](#52-分层文件系统overlayfs--预构建层--mvp)）。

验收标准：
- 沙箱内无法读取宿主机 `/etc/passwd` 等敏感文件
- 执行完毕后，可写层被自动彻底销毁

#### F3 — 系统调用过滤

集成 `libseccomp`，默认"拒绝所有"策略，仅开放白名单。后续演进为自适应 Profile（详见 [5.3](#53-自适应-seccomp-profile--phase-2)）。

验收标准：
- 阻止 `fork`、`execve`（防止进程爆炸）
- 默认断网模式，阻止网络相关调用
- 提供"安全网络模式"，允许仅特定域名/IP 访问

#### F4 — 资源额度控制

封装 Linux Cgroups v2 API，限制沙箱资源使用。

验收标准：
- 限制最大内存（如 128MB），超限自动 OOM Kill
- 限制最大 CPU 时间（如 5 秒），防止死循环

### 4.2 交互接口

#### F5 — HTTP API 服务

接口：`POST /execute`

请求体：
```json
{
  "language": "python",
  "code": "import math; print(math.sqrt(2))",
  "timeout_ms": 5000,
  "memory_limit_mb": 128
}
```

响应体：
```json
{
  "stdout": "1.4142135623...",
  "stderr": "",
  "exit_code": 0,
  "duration_ms": 12
}
```

#### F6 — 审计日志（差异化功能）

记录沙箱内进程的所有敏感行为，输出结构化日志：

| 记录项 | 示例 |
| :--- | :--- |
| 文件读写 | `READ /tmp/data.csv` |
| 网络连接 | `CONNECT 203.0.113.1:443` |
| 执行命令 | `EXEC python3 script.py` |

核心价值：让企业安全团队清楚知道 AI 在沙箱里做了什么。

---

## 5. 技术创新方案

> 按优先级排序。5.1 和 5.2 在 MVP 阶段实现，5.3 为 Phase 2 重点，5.4 和 5.5 为长期技术储备。

### 5.1 沙箱预热池 (Sandbox Pool) — MVP

**问题：** 每次请求都走 `clone → pivot_root → seccomp` 全流程，启动开销约 10ms。

**方案：** 预创建一批"空白沙箱"，请求到来时直接取用。

工作流程：
```
启动时：预创建 N 个沙箱（已完成 Namespace/FS/Seccomp 初始化）
         ↓
请求到来 → 从池中取一个就绪沙箱 → 注入代码执行 → 返回结果
         ↓
执行完毕 → 重置沙箱状态（清理可写层、重置环境变量）→ 回收至池中
```

关键设计：
- 池大小根据系统资源动态调整，支持自动扩缩容
- 池耗尽时自动降级为实时创建模式，保证可用性
- 类似 Firecracker snapshot/restore 思路，但在进程级别实现，开销远小于 MicroVM

预期效果：
- 冷启动从 ~10ms → **1-3ms**
- P99 延迟 < 5ms

### 5.2 分层文件系统（OverlayFS + 预构建层）— MVP

**问题：** Python 运行时数百 MB，每次创建临时文件系统复制运行时开销巨大；200 并发 × 128MB ≈ 25GB 内存压力不可接受。

**方案：** 采用 OverlayFS 分层架构，只读层共享、可写层隔离。

```
┌─────────────────────────────┐
│   可写层（用户代码 + 临时文件）  │  ← 每个沙箱独立，执行后销毁
├─────────────────────────────┤
│   只读层（Python runtime + 常用库）│  ← 所有沙箱共享，page cache 复用
└─────────────────────────────┘
```

关键设计：
- 预构建只读层包含 Python 解释器 + 常用库（numpy、pandas 等）
- 多个沙箱共享同一份只读层的 page cache
- 支持多语言运行时层的独立管理和版本切换

预期效果：
- 200 并发沙箱总内存开销 < 4GB（不含用户数据）
- 文件系统准备时间从"复制运行时"降至"创建空目录 + mount overlay"

### 5.3 自适应 Seccomp Profile — Phase 2

**问题：** 固定白名单要么太严（跑不起来），要么太松（有安全风险）。不同语言、不同库需要的系统调用差异巨大。

**方案：** 根据代码特征动态生成最小权限 Seccomp profile。竞品完全未涉足此领域。

```
用户代码 → 静态分析 import 语句 → 匹配 Profile 模板 → 生成最小权限 Seccomp 规则
                                      ↓
                          ┌──────────────────────┐
                          │ 纯计算 → 最严格 profile │
                          │ 文件 IO → 文件 profile  │
                          │ 网络访问 → 网络 profile  │
                          └──────────────────────┘
```

关键设计：
- 为每种语言运行时预定义 Profile 模板库
- 解析 import 语句自动匹配（如 `import numpy` → 放开 `mmap`/`mprotect`）
- 提供 `--learn` 模式：用 strace 跟踪执行，自动生成最小权限 profile 并缓存

预期效果：
- 系统调用白名单数量比固定方案减少 50%+
- Profile 生成延迟 < 1ms，不影响启动速度
- 从"一刀切白名单"进化为"最小权限原则的自动化实现"

### 5.4 基于 io_uring 的异步沙箱调度 — Phase 3

**问题：** 传统一请求一线程模式，并发上去后上下文切换开销大。

**方案：** 用 io_uring 做沙箱生命周期的异步管理，配合 Rust tokio 运行时。

关键设计：
- io_uring 异步管理进程创建、IO 收集、超时控制
- 单线程管理数百个并发沙箱
- batch submit 减少系统调用次数

预期效果：
- 并发能力从 200 QPS → **500+ QPS**（同等 4C8G 硬件）
- CPU 上下文切换减少 80%+

### 5.5 WASM 双引擎架构 — 长期方向

**问题：** 进程级隔离的安全强度不如 MicroVM，企业客户可能质疑。

**方案：** 引入 WebAssembly 作为第二执行引擎，根据代码特征自动路由。

```
用户代码 → 依赖分析
              ├── 无原生依赖 → WASM 引擎（Wasmtime）→ 接近 MicroVM 级隔离
              └── 有原生依赖（numpy 等）→ 原生引擎（进程隔离）→ 完整性能
```

预期效果：
- 直接回应"隔离不如 Firecracker"的质疑
- 纯逻辑代码启动速度可达亚毫秒级
- 为 Edge Computing 场景（浏览器端执行）打下基础

---

## 6. 安全架构与威胁模型

### 6.1 纵深防御体系

Synapse 采用四层纵深防御架构，任意单层被突破不会导致宿主机沦陷：

```
┌─────────────────────────────────────────────┐
│  第 1 层：用户权限隔离                          │
│  沙箱进程以 nobody (UID 65534) 运行            │
│  启用 User Namespace，容器内 root ≠ 宿主机 root │
├─────────────────────────────────────────────┤
│  第 2 层：Namespace 隔离                       │
│  PID / Mount / Network / IPC / UTS 全隔离     │
│  沙箱内看不到宿主机进程、网络、文件系统            │
├─────────────────────────────────────────────┤
│  第 3 层：Seccomp 系统调用过滤                  │
│  默认拒绝所有，仅白名单放行                      │
│  阻止 fork/execve/socket 等危险调用             │
├─────────────────────────────────────────────┤
│  第 4 层：Cgroups 资源限制                      │
│  内存/CPU/PID 数量硬上限                        │
│  防止资源耗尽型 DoS 攻击                        │
└─────────────────────────────────────────────┘
```

### 6.2 威胁矩阵

| 威胁类型 | 攻击示例 | 防御层 | 防御措施 |
| :--- | :--- | :--- | :--- |
| 沙箱逃逸 | 利用内核漏洞（如 Dirty Pipe）提权 | 第 1 层 + 第 3 层 | User Namespace 映射 + Seccomp 阻止相关系统调用 |
| 进程爆炸 | `fork()` 无限创建子进程 | 第 3 层 + 第 4 层 | Seccomp 阻止 fork；Cgroups 限制 PID 数量上限（如 10） |
| 资源耗尽 | 死循环 / 内存泄漏 | 第 4 层 | CPU 时间硬限制 + 内存 OOM Kill |
| 文件系统穿越 | `../../etc/passwd` 路径遍历 | 第 2 层 | pivot_root 切换根目录，OverlayFS 只读层不可写 |
| 网络外联 | `curl` 外传敏感数据 | 第 2 层 + 第 3 层 | Network Namespace 隔离 + Seccomp 阻止 socket 调用 |
| 信息泄露 | 读取 `/proc` 获取宿主机信息 | 第 2 层 | PID Namespace 隔离，挂载独立 procfs |
| 提权攻击 | `setuid` / `capset` 提升权限 | 第 1 层 + 第 3 层 | 无 capabilities 授予 + Seccomp 阻止提权调用 |

### 6.3 已知安全边界（诚实声明）

以下场景超出 Synapse 当前防御范围，需用户知悉：

- **内核零日漏洞：** 进程级隔离共享宿主机内核，内核零日漏洞理论上可逃逸。缓解措施：保持内核更新 + Seccomp 收窄攻击面。长期方案：WASM 双引擎（见 [5.5](#55-wasm-双引擎架构--长期方向)）
- **侧信道攻击：** 共享 CPU 缓存可能泄露信息（如 Spectre）。缓解措施：Cgroups CPU 隔离 + 沙箱生命周期极短（秒级销毁）
- **宿主机配置错误：** 如果宿主机内核未启用相关安全特性，防御层会降级。缓解措施：启动时自检内核配置，不满足最低要求则拒绝启动

### 6.4 安全测试要求

| 测试类型 | 内容 | 频率 |
| :--- | :--- | :--- |
| 逃逸测试 | 经典 CVE 复现（Dirty Pipe、Dirty COW 等） | 每个版本发布前 |
| 模糊测试 | 对 Seccomp profile 和 API 输入做 fuzz | 持续集成 |
| 渗透测试 | 邀请外部安全研究员做黑盒测试 | 每季度一次 |
| 合规审计 | 生成安全配置报告，供企业客户评估 | 按需 |

---

## 7. 错误处理与边界场景

### 7.1 错误分类与响应

所有错误通过统一的 API 响应格式返回：

```json
{
  "stdout": "",
  "stderr": "...",
  "exit_code": -1,
  "duration_ms": 5000,
  "error": {
    "type": "TIMEOUT",
    "message": "Execution exceeded 5000ms limit",
    "detail": "Process killed by SIGKILL after timeout"
  }
}
```

### 7.2 边界场景处理矩阵

| 场景 | 触发条件 | 处理策略 | 返回的 error.type |
| :--- | :--- | :--- | :--- |
| 执行超时 | 代码运行超过 `timeout_ms` | SIGKILL 强杀进程，回收沙箱 | `TIMEOUT` |
| 内存超限 | 内存使用超过 `memory_limit_mb` | Cgroups OOM Kill，回收沙箱 | `OOM_KILLED` |
| 死循环 | CPU 时间耗尽 | 等同超时处理 | `TIMEOUT` |
| 非法系统调用 | 触发 Seccomp 拦截 | 进程被 SIGSYS 终止 | `SYSCALL_BLOCKED` |
| 沙箱池耗尽 | 所有预热沙箱已被占用 | 降级为实时创建模式（延迟升高）；若实时创建也失败，返回 503 | `POOL_EXHAUSTED` / `SERVICE_UNAVAILABLE` |
| 语言不支持 | 请求了未安装的运行时 | 直接拒绝，不创建沙箱 | `UNSUPPORTED_LANGUAGE` |
| 代码为空 | `code` 字段为空字符串 | 直接拒绝 | `INVALID_INPUT` |
| 输出过大 | stdout/stderr 超过上限（默认 1MB） | 截断输出，标记 truncated | `OUTPUT_TRUNCATED` |

### 7.3 资源回收保证

无论执行成功或失败，必须保证：
- 沙箱进程在 `timeout_ms + 1000ms` 内被彻底清理（SIGKILL 兜底）
- 可写层文件系统在沙箱回收时同步销毁
- Cgroups 资源组在沙箱销毁后立即释放
- 异常沙箱（无法正常回收）标记为 `poisoned`，不再复用，后台异步清理

---

## 8. 多租户与并发模型

### 8.1 租户隔离架构

```
                    ┌─────────────┐
                    │  HTTP API   │
                    │  (tokio)    │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │  请求路由层   │  ← API Key 鉴权，识别租户
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │ 租户 A   │ │ 租户 B   │ │ 租户 C   │
        │ 配额管理  │ │ 配额管理  │ │ 配额管理  │
        └────┬─────┘ └────┬─────┘ └────┬─────┘
             ▼            ▼            ▼
        ┌─────────────────────────────────────┐
        │         全局沙箱预热池                 │
        │   （按租户配额分配，公平调度）           │
        └─────────────────────────────────────┘
```

### 8.2 租户配额模型

| 配额维度 | 默认值 | 说明 |
| :--- | :--- | :--- |
| 最大并发沙箱数 | 20 | 单租户同时运行的沙箱上限 |
| 每分钟请求数 (RPM) | 60 | 防止单租户打满系统 |
| 单次执行超时 | 30s | 可在请求中覆盖，但不超过此上限 |
| 单次内存上限 | 256MB | 可在请求中覆盖，但不超过此上限 |
| 每日执行总量 | 10,000 | 按计费套餐调整 |

### 8.3 公平调度策略

- **沙箱分配：** 预热池全局共享，但每个租户有并发上限。租户 A 不会因为占满池子而饿死租户 B
- **排队机制：** 当租户并发达到上限时，新请求进入租户级队列，FIFO 排队，队列满则返回 429 Too Many Requests
- **优先级：** 付费租户可配置更高优先级，优先从池中获取沙箱（Phase 3 实现）

---

## 9. 可观测性

### 9.1 指标体系 (Metrics)

通过 Prometheus 格式暴露，`GET /metrics` 端点：

| 指标名 | 类型 | 说明 |
| :--- | :--- | :--- |
| `synapse_sandbox_pool_size` | Gauge | 当前预热池中可用沙箱数 |
| `synapse_sandbox_pool_capacity` | Gauge | 预热池总容量 |
| `synapse_sandbox_active` | Gauge | 当前正在执行的沙箱数 |
| `synapse_request_duration_ms` | Histogram | 请求端到端延迟分布 |
| `synapse_request_total` | Counter | 请求总数（按 status/language 分标签） |
| `synapse_sandbox_oom_kills` | Counter | OOM Kill 次数 |
| `synapse_sandbox_timeouts` | Counter | 超时次数 |
| `synapse_sandbox_syscall_blocks` | Counter | Seccomp 拦截次数 |
| `synapse_sandbox_recycle_errors` | Counter | 沙箱回收失败次数（poisoned） |

### 9.2 结构化日志

所有日志输出 JSON 格式，便于 ELK/Loki 等系统采集：

```json
{
  "timestamp": "2024-07-15T10:30:00Z",
  "level": "INFO",
  "event": "sandbox_execute",
  "tenant_id": "tenant_abc",
  "sandbox_id": "sb_12345",
  "language": "python",
  "duration_ms": 12,
  "exit_code": 0,
  "memory_peak_mb": 45,
  "syscalls_blocked": 0
}
```

### 9.3 告警规则（建议）

| 告警 | 条件 | 严重级别 |
| :--- | :--- | :--- |
| 池水位过低 | `pool_size / pool_capacity < 20%` 持续 1 分钟 | Warning |
| 池完全耗尽 | `pool_size == 0` 持续 30 秒 | Critical |
| OOM 频率异常 | OOM Kill 速率 > 10/min | Warning |
| Seccomp 拦截激增 | 拦截速率突增 5 倍 | Critical（可能有攻击） |
| 沙箱回收失败 | poisoned 沙箱数 > 5 | Critical |

---

## 10. SDK 与集成方案

### 10.1 集成方式路线图

| 阶段 | 集成方式 | 说明 |
| :--- | :--- | :--- |
| MVP | HTTP REST API | `POST /execute`，最简集成 |
| Phase 2 | Python SDK | `pip install synapse-sdk`，封装 HTTP 调用 + 重试 + 连接池 |
| Phase 2 | WebSocket 长连接 | 支持流式输出（逐行返回 stdout），适合交互式场景 |
| Phase 3 | Node.js SDK | `npm install @synapse/sdk` |
| Phase 3 | gRPC 接口 | 高性能场景，减少序列化开销 |
| Phase 4 | OpenAI Function Calling 适配 | 直接作为 AI Agent 的 tool 使用 |

### 10.2 Python SDK 示例（Phase 2 目标）

```python
from synapse import Sandbox

sandbox = Sandbox(api_key="sk-xxx", base_url="http://localhost:8080")

result = sandbox.execute(
    language="python",
    code="import math; print(math.sqrt(2))",
    timeout_ms=5000,
    memory_limit_mb=128,
)

print(result.stdout)       # "1.4142135623..."
print(result.duration_ms)  # 12
```

### 10.3 WebSocket 流式输出示例（Phase 2 目标）

```
Client → WS /execute/stream
         {"language": "python", "code": "for i in range(5): print(i)"}

Server → {"type": "stdout", "data": "0\n"}
Server → {"type": "stdout", "data": "1\n"}
Server → {"type": "stdout", "data": "2\n"}
Server → {"type": "stdout", "data": "3\n"}
Server → {"type": "stdout", "data": "4\n"}
Server → {"type": "done", "exit_code": 0, "duration_ms": 15}
```

---

## 11. 语言运行时管理

### 11.1 Runtime Image 架构

```
synapse runtime list
┌──────────────────────────────────────────────┐
│  NAME              VERSION    SIZE    STATUS │
│  python-base       3.11.9     85MB   active │
│  python-scientific 3.11.9     320MB  active │
│  nodejs-base       20.15      55MB   active │
│  shell-alpine      3.20       8MB    active │
└──────────────────────────────────────────────┘
```

### 11.2 Runtime 定义文件（Synapsefile）

用户可通过声明式配置文件自定义运行时，类似 Dockerfile 但更轻量：

```yaml
# Synapsefile
name: python-scientific
base: python:3.11-slim
install:
  - numpy==1.26.4
  - pandas==2.2.0
  - matplotlib==3.8.0
env:
  PYTHONDONTWRITEBYTECODE: "1"
limits:
  max_memory_mb: 256
  max_timeout_ms: 30000
```

构建命令：
```bash
synapse runtime build -f Synapsefile
```

### 11.3 运行时生命周期

| 操作 | 命令 | 说明 |
| :--- | :--- | :--- |
| 构建 | `synapse runtime build` | 根据 Synapsefile 构建只读层 |
| 列表 | `synapse runtime list` | 查看已安装的运行时 |
| 更新 | `synapse runtime update <name>` | 重新构建并热替换（不中断服务） |
| 删除 | `synapse runtime remove <name>` | 删除运行时（需无活跃沙箱引用） |
| 导入/导出 | `synapse runtime export/import` | 离线环境下的运行时分发 |

### 11.4 预构建运行时仓库（Phase 3）

提供官方维护的运行时镜像仓库，用户可直接拉取：

```bash
synapse runtime pull python-scientific:latest
synapse runtime pull nodejs-base:20
```

---

## 12. 非功能性需求

| 维度 | 指标 | 说明 | 验证状态 |
| :--- | :--- | :--- | :--- |
| 安全性 | 沙箱逃逸防护 | 四层纵深防御，详见 [第 6 章](#6-安全架构与威胁模型) | ✅ 已有逃逸测试套件 |
| 并发性能 | ≥ 200 QPS (MVP) / 500+ QPS (Phase 3) | 单机 4C8G 配置 | ⚠️ 待 HTTP 负载测试验证 |
| 冷启动 | P99 < 50ms (MVP) / P99 < 5ms（预热池） | 预热池就绪后大幅降低 | ⚠️ 待 HTTP 负载测试验证 |
| 可用性 | 99.9%（SaaS 模式） | 沙箱池降级 + 健康检查保证 | 🔴 待 SaaS 部署 |
| 系统依赖 | 仅 Linux 内核 > 4.18 | 无需 Docker、K8s 等重型依赖 | ✅ 已验证 |
| 分发方式 | 单一静态链接二进制 | 支持 `x86_64` 和 `arm64` | ✅ 已支持 x86_64 |

> **注**：性能指标（QPS、冷启动延迟）目前仅有 criterion 微基准测试数据，尚未进行端到端 HTTP 负载测试验证。建议在 Phase 2 完成 HTTP 压测后更新此表。

---

## 13. 商业模式与定价策略

### 13.1 双轨模式

| 模式 | 目标客户 | 交付方式 |
| :--- | :--- | :--- |
| SaaS（云服务） | 中小团队、独立开发者 | 托管 API，按量付费 |
| 私有化部署 | 企业客户（金融/医疗/政府） | 单二进制文件 + License Key |

### 13.2 SaaS 定价（按量付费）

| 套餐 | 月费 | 包含执行次数 | 超出单价 | 并发上限 | 特性 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| Free | ¥0 | 1,000 次/月 | 不可超出 | 5 | 社区支持 |
| Pro | ¥99 | 50,000 次/月 | ¥0.002/次 | 50 | 优先队列 + 审计日志 |
| Team | ¥499 | 500,000 次/月 | ¥0.001/次 | 200 | 多成员 + 自定义运行时 |
| Enterprise | 定制 | 不限 | 定制 | 定制 | SLA + 专属支持 |

计量维度：
- 按执行次数计费（一次 API 调用 = 一次执行）
- 超时/OOM 的执行也计费（已消耗资源）
- 未来考虑按 CPU 时间计费（更公平，Phase 4）

### 13.3 私有化部署定价

| 套餐 | 年费 | 节点数 | 特性 |
| :--- | :--- | :--- | :--- |
| Standard | ¥20,000 | 单节点 | 基础功能 + 邮件支持 |
| Professional | ¥80,000 | ≤ 5 节点 | 全功能 + 审计日志 + 工单支持 |
| Enterprise | 定制 | 不限 | 定制开发 + 驻场支持 + SLA |

---

## 14. 开源策略

### 14.1 Open Core 模式

采用"核心引擎开源 + 企业功能闭源"的 Open Core 商业模式，参考 GitLab CE/EE、Grafana OSS/Enterprise 的成熟实践。

```
┌─────────────────────────────────────────────────────┐
│                  Synapse Enterprise (闭源)            │
│                                                      │
│  审计日志 · 多租户管理 · SaaS 控制台 · License 管理    │
│  优先级调度 · 高级告警 · SSO/LDAP · 合规报告           │
├─────────────────────────────────────────────────────┤
│                  Synapse Core (开源, Apache 2.0)      │
│                                                      │
│  沙箱引擎 · Namespace/Seccomp/Cgroups 隔离            │
│  沙箱预热池 · OverlayFS 分层 · HTTP API               │
│  自适应 Seccomp · 运行时管理 · 基础 Metrics           │
└─────────────────────────────────────────────────────┘
```

### 14.2 开源 vs 闭源边界

> **注**：以下标注当前实现状态。部分原计划 Enterprise 专属功能已在开源版本中实现。

| 功能 | 开源 (Core) | 闭源 (Enterprise) | 实现状态 |
| :--- | :--- | :--- | :--- |
| 沙箱引擎（Namespace/Seccomp/Cgroups） | ✅ | ✅ | ✅ 已实现 |
| 沙箱预热池 | ✅ | ✅ | ✅ 已实现 |
| OverlayFS 分层文件系统 | ✅ | ✅ | ✅ 已实现 |
| 自适应 Seccomp Profile | ✅ | ✅ | 🔴 待开发 |
| HTTP API (`POST /execute`) | ✅ | ✅ | ✅ 已实现 |
| Synapsefile 运行时管理 | ✅ | ✅ | ⚠️ 部分实现 |
| 基础 Metrics (`/metrics`) | ✅ | ✅ | ✅ 已实现 |
| Python / Node.js SDK | ✅ | ✅ | 🔴 待开发 |
| WebSocket 流式输出 | ✅ | ✅ | ⚠️ 基础实现 |
| 审计日志（细粒度行为记录） | ✅ | ✅ | ✅ 已实现（原计划 Enterprise） |
| 多租户管理 + 配额控制 | ✅ | ✅ | ✅ 已实现（原计划 Enterprise） |
| SaaS 控制台（注册/API Key/计费） | ❌ | ✅ | 🔴 待开发 |
| 优先级调度 | ❌ | ✅ | 🔴 待开发 |
| SSO / LDAP 集成 | ❌ | ✅ | 🔴 待开发 |
| 合规报告生成 | ❌ | ✅ | 🔴 待开发 |
| 高级告警规则 | ❌ | ✅ | 🔴 待开发 |
| License Key 管理 | ❌ | ✅ | 🔴 待开发 |

### 14.3 开源许可证选择

**推荐：Apache License 2.0**

选择理由：
- 允许商业使用，对企业用户友好，降低采用门槛
- 包含专利授权条款，保护贡献者和使用者
- 与 Rust 生态主流一致（tokio、serde 等均为 Apache 2.0 / MIT 双许可）
- 不要求衍生作品开源（区别于 GPL），不会吓跑企业客户

### 14.4 开源运营策略

| 阶段 | 动作 | 目标 |
| :--- | :--- | :--- |
| MVP 发布 | GitHub 开源 Core 版本，写技术博客讲解架构 | 建立技术影响力，吸引早期用户 |
| Phase 2 | 接受社区 PR，建立 Contributor Guide | 培养社区贡献者，降低维护成本 |
| Phase 3 | 发布 Enterprise 版本，开源版保持活跃更新 | 开始商业化变现 |
| 长期 | 定期将 Enterprise 成熟功能下放到 Core | 保持开源版吸引力，避免社区流失 |

### 14.5 社区建设

- **文档：** 从 Day 1 就维护高质量的 README、Architecture Doc、Contributing Guide
- **示例：** 提供 `examples/` 目录，包含常见集成场景（FastAPI + Synapse、LangChain + Synapse 等）
- **Benchmark：** 提供可复现的性能测试脚本，让用户自己跑对比（vs Docker、vs E2B）
- **Discord/GitHub Discussions：** 建立社区交流渠道
- **安全响应：** 建立 SECURITY.md，明确漏洞报告流程和响应 SLA

---

## 15. 测试策略

### 15.1 测试金字塔

```
                    ╱╲
                   ╱  ╲
                  ╱ E2E ╲          少量端到端测试
                 ╱────────╲        验证完整用户流程
                ╱ 集成测试  ╲       中等数量
               ╱────────────╲      验证模块间交互
              ╱   单元测试    ╲     大量
             ╱────────────────╲    验证单个函数/模块
            ╱   安全专项测试    ╲   持续运行
           ╱────────────────────╲  逃逸/fuzz/渗透
          ╱    性能基准测试       ╲ 每次发布
         ╱────────────────────────╲ 防止性能回退
```

### 15.2 单元测试

覆盖核心引擎的每个模块，使用 Rust 内置测试框架 + `#[cfg(test)]`。

| 模块 | 测试重点 | 覆盖率目标 |
| :--- | :--- | :--- |
| `namespace` | clone flags 正确性、各 Namespace 隔离验证 | ≥ 90% |
| `seccomp` | Profile 加载、系统调用拦截、白名单/黑名单切换 | ≥ 95% |
| `cgroups` | 资源限制设置、OOM Kill 触发、资源组清理 | ≥ 90% |
| `filesystem` | OverlayFS 挂载/卸载、pivot_root、可写层清理 | ≥ 90% |
| `sandbox_pool` | 池创建/获取/回收/扩缩容、降级逻辑、poisoned 处理 | ≥ 95% |
| `api` | 请求解析、参数校验、错误响应格式 | ≥ 90% |
| `runtime` | Synapsefile 解析、运行时构建/列表/删除 | ≥ 85% |

示例：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seccomp_blocks_fork() {
        let profile = SeccompProfile::default_whitelist();
        let result = profile.check_syscall(libc::SYS_fork);
        assert_eq!(result, SeccompAction::Kill);
    }

    #[test]
    fn test_seccomp_allows_write() {
        let profile = SeccompProfile::default_whitelist();
        let result = profile.check_syscall(libc::SYS_write);
        assert_eq!(result, SeccompAction::Allow);
    }

    #[test]
    fn test_sandbox_pool_acquire_and_release() {
        let pool = SandboxPool::new(PoolConfig { initial_size: 5, max_size: 10 });
        let sandbox = pool.acquire().expect("should get a sandbox");
        assert_eq!(pool.available(), 4);
        pool.release(sandbox);
        assert_eq!(pool.available(), 5);
    }
}
```

### 15.3 集成测试

验证多个模块协同工作的正确性，需要 Linux 环境（CI 中使用特权容器或裸机）。

| 测试场景 | 验证内容 | 运行条件 |
| :--- | :--- | :--- |
| 完整沙箱生命周期 | 创建 → 执行代码 → 收集输出 → 销毁 | 需要 root 权限 |
| Namespace 隔离验证 | 沙箱内 `getpid()` 返回 1；无法看到宿主机进程 | 需要 root 权限 |
| 文件系统隔离验证 | 沙箱内无法访问 `/etc/passwd`；可写层执行后被清理 | 需要 root 权限 |
| Seccomp 拦截验证 | 执行 `os.fork()` 的 Python 代码，验证被 SIGSYS 终止 | 需要 root 权限 |
| Cgroups 限制验证 | 执行内存分配代码，验证 OOM Kill 触发 | 需要 root 权限 |
| 超时处理验证 | 执行 `while True: pass`，验证超时后进程被清理 | 需要 root 权限 |
| 沙箱池并发验证 | 50 个并发请求，验证池分配/回收无竞态 | 需要 root 权限 |
| API 错误处理 | 空代码、不支持的语言、超大输出等边界输入 | 无特殊要求 |

CI 配置要点：
```yaml
# .github/workflows/integration.yml
jobs:
  integration-test:
    runs-on: ubuntu-latest
    container:
      image: rust:latest
      options: --privileged  # 需要特权模式运行 Namespace/Cgroups
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --test integration -- --test-threads=1
```

### 15.4 安全专项测试

这是 Synapse 最关键的测试类别，直接关系产品可信度。

#### 15.4.1 沙箱逃逸测试套件

针对已知 CVE 和常见攻击手法，构建自动化逃逸测试：

| 测试用例 | 攻击手法 | 预期结果 |
| :--- | :--- | :--- |
| `escape_fork_bomb` | `fork()` 无限创建子进程 | Seccomp 拦截 或 Cgroups PID 限制触发 |
| `escape_path_traversal` | `open("../../etc/shadow")` | 被 pivot_root 阻止，返回 ENOENT |
| `escape_mount_proc` | `mount("proc", "/proc", ...)` | Seccomp 拦截 mount 调用 |
| `escape_network` | `socket(AF_INET, ...)` + `connect(...)` | Seccomp 拦截 或 Network NS 隔离 |
| `escape_setuid` | `setuid(0)` 尝试提权 | Seccomp 拦截 + User NS 映射阻止 |
| `escape_ptrace` | `ptrace(PTRACE_ATTACH, 1)` | Seccomp 拦截 ptrace 调用 |
| `escape_kernel_exploit` | 模拟 Dirty Pipe (CVE-2022-0847) | Seccomp 拦截 splice 调用（在受影响内核版本上测试） |
| `escape_symlink` | 创建符号链接指向宿主机文件 | pivot_root 限制，符号链接无法逃逸 |

```rust
#[test]
fn escape_fork_bomb() {
    let result = execute_in_sandbox(r#"
import os
while True:
    os.fork()
"#);
    assert!(
        result.error_type == "SYSCALL_BLOCKED" ||
        result.exit_code != 0
    );
    // 验证宿主机进程数未异常增长
    assert!(host_process_count() < MAX_EXPECTED_PROCESSES);
}
```

#### 15.4.2 模糊测试 (Fuzzing)

使用 `cargo-fuzz` + `libFuzzer` 对关键输入做模糊测试：

| Fuzz 目标 | 输入 | 目标 |
| :--- | :--- | :--- |
| Seccomp Profile 解析 | 随机 profile 配置 | 不 panic、不产生过于宽松的规则 |
| API 请求解析 | 随机 JSON payload | 不 panic、正确返回错误 |
| Synapsefile 解析 | 随机 YAML 内容 | 不 panic、不产生危险配置 |
| 代码静态分析器 | 随机 Python 代码 | 不 panic、不误判 import |

```bash
# 运行 fuzz 测试
cargo fuzz run fuzz_seccomp_profile -- -max_total_time=3600
cargo fuzz run fuzz_api_request -- -max_total_time=3600
```

### 15.5 性能基准测试 (Benchmark)

使用 `criterion` 框架，每次发布前运行，防止性能回退。

#### 15.5.1 核心指标基准

| 基准测试 | 测量内容 | 目标值 | 回退阈值 |
| :--- | :--- | :--- | :--- |
| `bench_sandbox_create` | 从零创建沙箱的耗时 | < 10ms | 回退 > 20% 则 CI 失败 |
| `bench_sandbox_pool_acquire` | 从预热池获取沙箱的耗时 | < 3ms | 回退 > 20% 则 CI 失败 |
| `bench_execute_hello_world` | 执行 `print("hello")` 端到端 | < 20ms | 回退 > 20% 则 CI 失败 |
| `bench_execute_numpy` | 执行 numpy 计算端到端 | < 100ms | 回退 > 30% 则 CI 失败 |
| `bench_sandbox_recycle` | 沙箱回收 + 重置耗时 | < 5ms | 回退 > 20% 则 CI 失败 |
| `bench_seccomp_profile_gen` | 自适应 Profile 生成耗时 | < 1ms | 回退 > 50% 则 CI 失败 |

```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_sandbox_pool_acquire(c: &mut Criterion) {
    let pool = SandboxPool::new(PoolConfig::default());
    c.bench_function("pool_acquire", |b| {
        b.iter(|| {
            let sb = pool.acquire().unwrap();
            pool.release(sb);
        })
    });
}

criterion_group!(benches, bench_sandbox_pool_acquire);
criterion_main!(benches);
```

#### 15.5.2 压力测试

使用 `wrk` 或 `k6` 对 HTTP API 做压力测试：

```bash
# 200 并发，持续 60 秒
wrk -t4 -c200 -d60s -s execute.lua http://localhost:8080/execute

# execute.lua
wrk.method = "POST"
wrk.headers["Content-Type"] = "application/json"
wrk.body = '{"language":"python","code":"print(1)","timeout_ms":5000,"memory_limit_mb":128}'
```

验收标准（4C8G 单机）：

| 指标 | MVP 目标 | Phase 3 目标 |
| :--- | :--- | :--- |
| QPS | ≥ 200 | ≥ 500 |
| P50 延迟 | < 15ms | < 8ms |
| P99 延迟 | < 50ms | < 20ms |
| 错误率 | < 0.1% | < 0.01% |
| 内存使用 | < 4GB | < 4GB |

#### 15.5.3 性能回退检测

集成到 CI，每次 PR 自动对比 main 分支的 benchmark 结果：

```yaml
# .github/workflows/benchmark.yml
jobs:
  benchmark:
    runs-on: ubuntu-latest
    container:
      image: rust:latest
      options: --privileged
    steps:
      - uses: actions/checkout@v4
      - run: cargo bench -- --output-format bencher | tee output.txt
      - uses: benchmark-action/github-action-benchmark@v1
        with:
          tool: cargo
          output-file-path: output.txt
          alert-threshold: "120%"       # 性能下降 20% 则告警
          fail-on-alert: true           # 超过阈值则 CI 失败
          comment-on-alert: true        # 在 PR 中评论性能变化
```

### 15.6 端到端测试 (E2E)

模拟真实用户场景，验证完整链路：

| 场景 | 步骤 | 验证 |
| :--- | :--- | :--- |
| 基础执行 | API 发送 Python 代码 → 获取结果 | stdout 正确、exit_code = 0 |
| 超时处理 | 发送死循环代码 → 等待超时 | 返回 TIMEOUT 错误、沙箱被清理 |
| OOM 处理 | 发送大内存分配代码 → 触发 OOM | 返回 OOM_KILLED、资源被释放 |
| 并发安全 | 50 个并发请求 → 全部返回 | 无竞态、无数据串扰 |
| 沙箱隔离 | 沙箱 A 写文件 → 沙箱 B 读同路径 | 沙箱 B 读不到沙箱 A 的文件 |
| 池降级 | 耗尽预热池 → 继续发请求 | 降级为实时创建，请求仍成功（延迟升高） |
| 运行时切换 | 先执行 Python → 再执行 Node.js | 两者均正确返回 |

### 15.7 CI/CD 流水线总览

```
┌─────────────────────────────────────────────────────────────┐
│                        PR 提交触发                           │
├──────────┬──────────┬──────────┬──────────┬────────────────┤
│  代码检查  │ 单元测试  │ 集成测试  │ 安全测试  │  性能基准测试   │
│          │          │          │          │               │
│ clippy   │ cargo    │ 特权容器  │ 逃逸测试  │ criterion     │
│ fmt      │ test     │ 中运行    │ fuzz     │ + 回退检测     │
│ audit    │          │          │ (定时)    │               │
├──────────┴──────────┴──────────┴──────────┴────────────────┤
│                      全部通过                                │
├─────────────────────────────────────────────────────────────┤
│                    合并到 main                               │
├──────────┬──────────────────────────────────────────────────┤
│ 构建二进制 │  x86_64 + arm64 静态链接                         │
├──────────┤                                                  │
│ E2E 测试  │  完整用户场景验证                                  │
├──────────┤                                                  │
│ 压力测试   │  wrk/k6 验证 QPS 和延迟（定时，非每次）             │
├──────────┴──────────────────────────────────────────────────┤
│                    发布 Release                              │
└─────────────────────────────────────────────────────────────┘
```

### 15.8 测试环境要求

| 环境 | 用途 | 配置 |
| :--- | :--- | :--- |
| CI (GitHub Actions) | 单元测试 + 集成测试 + 安全测试 | ubuntu-latest, --privileged, Linux 内核 > 4.18 |
| 性能测试机 | 基准测试 + 压力测试 | 专用 4C8G 裸机，避免虚拟化噪声 |
| 安全测试机 | 逃逸测试 + 渗透测试 | 可切换内核版本（测试特定 CVE），物理隔离 |

---

## 16. 开发路线图

> **当前状态**：Phase 1 (MVP) 已完成，正在向 Phase 2 过渡。部分原计划 Phase 2/3 的功能已提前实现（审计日志、多租户配额）。

### Phase 1：核心验证 (MVP) — 已完成 ✅

```
目标：跑通"代码进去、结果出来"的完整链路，验证 < 5ms 启动承诺
状态：已完成（2026-03）
```

- [x] 进程隔离（Namespace: PID / Mount / Network / IPC / UTS）
- [x] 文件系统隔离（pivot_root + OverlayFS 分层架构）
- [x] Seccomp 过滤（黑名单模式）
- [x] 沙箱预热池（Sandbox Pool）
- [x] Python 语言执行支持
- [x] 基础错误处理（超时 / OOM / 非法调用）
- [x] 基础 Metrics 端点（`/metrics`）
- [x] 单元测试 + 集成测试 + 逃逸测试套件
- [x] 性能基准测试（criterion）+ 发布门禁脚本
- [x] GitHub 开源 Core 版本（Apache 2.0）

**已提前实现的功能（原计划 Phase 2/3）：**

- [x] 审计日志功能（细粒度行为记录、syscall 审计）
- [x] 多租户配额管理（并发限制、速率限制、公平调度）
- [x] 全局执行队列与背压机制
- [x] 资源限制（CPU 时间独立于 wall timeout）
- [x] Runtime 版本管理（CLI：runtime list/verify/install/import-host/activate）

### Phase 2：安全加固与开发者体验 — 进行中

```
目标：白名单 Seccomp + SDK + 错误模型稳定，建立安全壁垒和开发者生态
预计周期：4-6 周
```

- [x] Cgroups v2 资源限制（内存 / CPU / PID）
- [x] 断网模式（Network Namespace 隔离）
- [ ] Seccomp 切换为白名单模式
- [ ] 自适应 Seccomp Profile（静态分析 + 模板库 + learn 模式）
- [ ] Python SDK 发布
- [x] WebSocket 流式输出（基础实现，需完善）
- [x] 多租户配额管理（已完成）
- [ ] Fuzz 测试集成到 CI
- [ ] 错误模型与 API 契约冻结
- [ ] HTTP 负载测试与性能验证

**Phase 2 当前阻塞项：**

| 项目 | 状态 | 备注 |
|-----|------|-----|
| Python SDK | 待开发 | 无开发者工具包 |
| 白名单 Seccomp | 待开发 | 当前为黑名单模式 |
| 性能验证 | 待验证 | 无 HTTP 负载测试数据 |
| 错误模型 | 部分完成 | 缺少细分内部错误类别 |

### Phase 3：商业化与多运行时 — 待启动

```
目标：多语言 + SaaS 控制台，正式商业化
预计周期：4-8 周
```

- [ ] Node.js / Shell 脚本执行支持
- [ ] Synapsefile 运行时管理 + 官方运行时仓库
- [ ] SaaS 控制台（注册、API Key、用量统计、计费）
- [ ] io_uring 异步调度（目标 500+ QPS）
- [ ] Node.js SDK 发布
- [ ] 压力测试自动化（wrk/k6）
- [ ] 发布 Enterprise 版本
- [ ] 正式上线 Beta 版

### Phase 4：长期技术演进

```
目标：WASM 双引擎 + 更多语言 + 生态拓展
```

- [ ] WASM 双引擎架构（Wasmtime 集成 + 智能路由）
- [ ] gRPC 接口
- [ ] OpenAI Function Calling 适配
- [ ] Edge Computing 场景探索
- [ ] 按 CPU 时间计费模式
- [ ] 更多语言运行时（Go / Java / Ruby）
- [ ] 外部渗透测试 + 安全认证
