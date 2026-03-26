# Synapse 功能补齐计划（P0+P1）

> 目标是在现有基础上补齐“广泛用户开箱即用”的功能闭环：首次安装可跑通、公开部署可控、错误可自动处理、接入成本低、性能与稳定性可持续验证。

## Implementation Status

- `已完成` P0-1 `synapse init`：新增一键初始化命令，复用 `doctor` 检查链路，阻塞缺失依赖时给出修复建议，并在成功后输出服务启动与健康检查下一步。
- `已完成` P0-2 runtime 自举：支持默认 bundle/host import 自举，active runtime 会标记安装来源，`runtime verify`/`doctor`/`init` 会显示 `bundle` 或 `host import`。
- `已完成` P0-3 API 认证与访问控制：`/execute`、`/execute/stream`、`/audits/:request_id`、`/metrics` 已接入统一 Bearer token 中间件，并增加 token 绑定 tenant 范围校验。
- `部分完成` P0-4 错误模型细化：已补齐 `auth_required`、`auth_invalid`、`tenant_forbidden` 等外部错误码；其余运行时/资源类错误码仍沿用现有命名，后续可继续拆分。
- `已完成` P1-5 可观测性：新增执行生命周期 telemetry，覆盖 `admitted`、`queued`、`started`、`runtime_resolved`、`limit_hit`、`completed`、`cleanup_done`，并补充 metrics 与 audit smoke 测试。
- `已完成` P1-6 Python SDK：新增 `sdk/python` 最小客户端，支持 `execute()`、`execute_stream()`、token、tenant 与基础错误映射，并提供同步/流式示例。
- `已完成` P1-7 性能门禁：扩展 Criterion 基线到 `pool_acquire`、`sandbox_create`、`execute_fast_path`、`sandbox_recycle`，并新增 `scripts/perf_gate.sh` 与 `scripts/http_bench.py`。
- `部分完成` P1-8 安全策略表达：已显式暴露 `network_policy` 接口，默认仍断网；`allow_list` 模式当前会稳定返回 `policy_blocked`，避免隐式失败。选择性放通白名单目标仍需要后续 sandbox backend 支持。

---

## Summary

当前仓库已经具备基础执行、CLI/API、审计、配额、调度、CI 和测试骨架，但距离“面向外部开发者直接试用的开源产品”仍有明显差距。

本计划聚焦功能补齐，不涵盖 README 双语、贡献指南、许可证整理、发布页包装等文档与治理项。功能目标分为两层：

- `P0`：首发必须具备，解决安装、自举、安全暴露、错误可编排四个问题。
- `P1`：增强可用性与产品成熟度，解决观测、接入、性能回归、安全策略表达能力的问题。

---

## Implementation Changes

### P0 首发必须功能

#### 1. 新增 `synapse init` 一键初始化命令

目标：把当前需要手工串联的依赖检查、runtime 导入和启动验证压缩成单个入口。

实现要点：

- 在 `synapse-cli` 中新增 `init` 子命令。
- 执行顺序固定为：
  1. 检查 Linux 平台与关键依赖：`bwrap`、`strace`、cgroup v2、临时目录可写、audit 目录可写。
  2. 检查是否存在可用 Python runtime。
  3. 若不存在 runtime，则尝试从默认 bundle 或宿主机导入。
  4. 验证 active runtime 是否可执行。
  5. 输出下一步命令：`synapse serve --listen ...` 与 `curl /health` 示例。
- `init` 必须是幂等的；重复执行不破坏已安装 runtime，也不覆盖用户已有配置。
- `doctor` 保留为底层检查工具，`init` 调用相同检查逻辑并给出更明确修复建议。

验收标准：

- 全新 Linux 环境中，用户执行一次 `synapse init` 后，可以直接进入服务启动与接口验证流程。
- 如果缺少关键依赖，`init` 必须明确指出缺失项、失败原因和修复方向。

#### 2. Runtime 自举能力补齐

目标：减少“必须手动准备 Python 解释器并理解 runtime store”的门槛。

实现要点：

- 明确默认 runtime 自举策略：
  - 第一优先级：导入仓库或发布资产附带的本地 runtime bundle。
  - 第二优先级：使用现有 `runtime import-host` 从宿主机 `python3` 导入。
- 在 `RuntimeRegistry` 之上增加“默认 runtime 就绪”检查与安装入口，供 `init` 调用。
- CLI 输出必须明确说明当前 active runtime 来源：`bundle` 或 `host import`。
- 对自举失败场景补齐结构化错误：bundle 缺失、bundle 校验失败、host python 缺失、runtime verify 失败。

验收标准：

- 用户不需要先理解 runtime store 结构，也能得到一个可用的 Python runtime。
- `runtime list` 和 `runtime verify` 能准确反映默认 runtime 的安装来源与状态。

#### 3. API 认证与访问控制

目标：让服务具备最小可公开部署能力，避免当前“知道地址即可调用”的状态。

实现要点：

- 为 `/execute`、`/execute/stream`、`/audits/:request_id`、`/metrics` 增加可配置 token 认证。
- 鉴权输入形式固定为 `Authorization: Bearer <token>`。
- 鉴权逻辑进入 API 层统一中间件，不分散在各 handler 内。
- 保持租户机制与 token 关系清晰：
  - `x-synapse-tenant-id` 继续保留。
  - token 至少要能绑定允许访问的 tenant 范围，防止跨租户读取 audit。
- 对未授权、token 无效、tenant 不匹配三类情况返回稳定错误码和 HTTP 状态。

验收标准：

- 未带 token 请求返回 `401`。
- 无效 token 返回 `401`。
- token 与 tenant 不匹配返回 `403`。
- audit 接口不能通过伪造 header 越权读取其他 tenant 数据。

#### 4. 错误模型继续细化

目标：让 SDK、自动化平台和调用方可以依据错误码做重试、提示和分类处理，而不是只拿到宽泛失败。

实现要点：

- 在现有 `SynapseError` / API error code 基础上继续拆分产品级错误：
  - `runtime_missing`
  - `runtime_invalid`
  - `sandbox_boot_failed`
  - `timeout_wall`
  - `timeout_cpu`
  - `oom_killed`
  - `policy_blocked`
  - `quota_rejected`
  - `auth_required`
  - `auth_invalid`
  - `tenant_forbidden`
  - `output_truncated`
- 保持响应结构稳定，优先扩展错误码和值域，不推翻现有响应外形。
- 为所有对外暴露错误补齐 API 测试。
- 明确哪些错误适合重试，哪些不应重试，并在 SDK 设计中复用这套分类。

验收标准：

- 常见失败场景都能映射到单一、稳定、可文档化的错误码。
- 不再依赖 `execution_failed` 这种过于宽泛的聚合错误处理主流程。

### P1 产品可用性增强

#### 5. 可观测性补齐

目标：让使用者和维护者能快速回答“请求发生了什么、卡在哪里、为什么失败”。

实现要点：

- 在执行生命周期增加结构化事件：
  - admitted
  - queued
  - started
  - runtime_resolved
  - limit_hit
  - completed
  - cleanup_done
- 统一 metrics 标签，至少覆盖：
  - 成功/失败
  - error_code
  - 是否排队
  - 是否截断输出
- 强化 request_id 贯穿日志、metrics 和 audit。
- 增加最小 smoke 验证，确保关键 metrics 和 tracing 字段确实被发出。

验收标准：

- 单次请求从进入到结束的关键节点都可通过日志或指标追踪。
- 常见故障可以通过 request_id 在日志和审计之间关联定位。

#### 6. Python SDK 与集成样例

目标：把接入方式从“手写 HTTP 请求”提升到“可直接嵌入应用/Agent”。

实现要点：

- 新增 `sdk/python` 目录，提供最小 Python SDK。
- 首版 SDK 范围固定为：
  - `execute()`
  - `execute_stream()`
  - 连接配置
  - token 认证
  - 基于错误码的基础异常映射
- 不在首版引入复杂抽象；优先保证 API 契约清晰、异常清晰、示例可跑。
- 增加 2 个最小集成示例：
  - 直接调用执行接口
  - 流式接收 stdout/stderr

验收标准：

- 外部 Python 开发者不需要自己拼接 HTTP 请求就能完成同步执行和流式执行。
- SDK 示例可作为 README 和集成文档的直接素材。

#### 7. 性能回归门禁

目标：把“很快”从口头承诺变成可复跑、可比较、可卡门禁的基线。

实现要点：

- 保留现有 Criterion benchmark，并扩展为稳定基线：
  - pool acquire
  - sandbox create
  - execute fast path
  - sandbox recycle
- 增加 HTTP 层可重复压测脚本，覆盖固定并发、固定 payload 的请求。
- 明确阈值：
  - P50 / P95 延迟
  - 错误率
  - 吞吐下限
- 先接入 release gate 或手动 gate，后续再视成本纳入 CI。

验收标准：

- 每次发布前都能复跑同一套性能验证。
- 出现明显性能回退时可以被 gate 拦截，而不是靠人工体感发现。

#### 8. 安全策略表达能力增强

目标：让默认安全策略之外，用户可以显式选择有限的网络能力和更清晰的策略模式。

实现要点：

- 保持默认断网模式不变。
- 增加受限网络模式的明确接口定义，支持显式白名单。
- 将安全策略暴露为清晰的 API/配置选项，而不是隐式实现细节。
- 补齐 seccomp / 网络策略相关错误返回，使策略阻断可被调用方识别。

验收标准：

- 默认模式下网络仍被阻断。
- 白名单模式下只允许显式配置的目标访问。
- 策略不满足时，调用方能收到清晰、稳定的错误码，而不是泛化执行失败。

---

## Public Interface Changes

- CLI 新增：`synapse init`
- CLI 调整：`synapse doctor` 输出修复建议更明确，可被 `init` 复用
- API 调整：受保护接口默认要求 `Authorization: Bearer <token>`
- API 调整：新增认证/鉴权相关错误码
- API 调整：错误模型继续细化，但保持响应结构向后兼容
- SDK 新增：`sdk/python` 最小 Python 客户端

---

## Test Plan

### 首次体验验收

- 在全新 Linux 环境执行 `synapse init`。
- 确认 `init` 能完成依赖检查与 runtime 准备，或明确输出修复建议。
- 执行 `synapse serve --listen 127.0.0.1:8080` 后，`curl /health` 返回成功。
- 发起最小 `/execute` 请求并拿到可预期响应。

### 安全与鉴权限收

- 未授权请求必须返回 `401`。
- 非法 token 必须返回 `401`。
- token 与 tenant 不匹配必须返回 `403`。
- audit 接口不能越权访问其他 tenant 的数据。

### 错误模型验收

- runtime 缺失、wall timeout、CPU 超时、OOM、策略阻断、配额拒绝、输出截断均有稳定错误码。
- API 与 SDK 对这些错误的分类保持一致。

### 可观测性验收

- 执行一次成功请求和一次失败请求，均能通过 request_id 串联日志、metrics、audit。
- 核心生命周期字段在日志或指标中可见。

### SDK 验收

- Python SDK 能完成同步执行。
- Python SDK 能完成流式执行。
- SDK 对认证失败和执行失败能映射到明确异常。

### 性能验收

- 复跑 benchmark 与 HTTP 压测脚本，得到固定格式结果。
- 性能结果满足预设阈值或在 gate 中被明确阻断。

---

## Assumptions

- 首发定位仍是“开发者试用版”，不是企业生产承诺版本。
- 首发平台限定 Linux，非 Linux 不纳入安全执行承诺。
- 默认只要求 Python 生态跑通，多语言支持不纳入本轮计划。
- 文档、README 双语、CONTRIBUTING、发布资产等非功能项单独推进，不混入本文件。
