<!-- Generated: 2026-03-25 | Scope: architecture refactor plan | Status: draft -->
# Synapse 架构改造计划

## 1. 目标

本计划的目标不是把项目改造成大型通用框架，而是把当前的单一执行域收敛成可强制约束的分层结构，满足以下要求：

- 业务域保持单一：代码执行沙箱。
- 依赖方向只能前进，不能反向依赖。
- 横切能力只能通过单一显式入口 `Providers` 进入。
- 系统能力与业务逻辑分离，避免 `executor.rs`、`pool.rs`、`server.rs` 继续承载过多职责。
- 未来加入认证、连接器、遥测、功能标志时，不污染核心执行链路。

## 2. 当前问题

当前工程能正常工作，但边界不够稳定：

- `synapse-core/src/executor.rs` 同时负责请求校验、语言解析、沙箱目录生命周期、脚本写入、进程启动、seccomp、内存限制、输出收集。
- `synapse-core/src/pool.rs` 同时负责预热池、租约管理、池回收、降级执行、指标统计。
- `synapse-api/src/server.rs` 同时负责路由、状态注入、环境配置读取、错误映射。
- `synapse-cli/src/main.rs` 直接做环境探测和运行前检查。
- 没有显式的 `Providers` 入口，横切能力通过 ambient 依赖散落在各处。

这会带来四个后果：

1. 依赖方向靠约定，不靠结构。
2. 安全边界难审计。
3. 新能力容易继续向核心逻辑扩散。
4. 自动化强制规则很难落地，因为当前实现本身就没有清晰层次。

## 3. 重构原则

### 3.1 保留的东西

- 保留当前三个 crate：`synapse-core`、`synapse-api`、`synapse-cli`。
- 保留当前单一业务域：执行代码。
- 保留现有的 API 形态：`GET /health`、`GET /metrics`、`POST /execute`。
- 保留现有安全策略的方向：Linux 必须通过 `bwrap` 路径执行，不再静默降级。

### 3.2 不做的东西

- 不引入数据库仓储层，除非未来确实出现持久化需求。
- 不做过度模块化，不为了形式拆出大量空壳 crate。
- 不在当前阶段引入完整插件系统。
- 不改造为与业务无关的通用应用框架。

### 3.3 必须达成的东西

- 业务代码不得直接读取环境变量、启动子进程、探测平台能力。
- 横切能力必须从 `Providers` 注入。
- `api` 只做运输层适配，不承载核心业务规则。
- `runtime` 只做系统能力，不直接处理 HTTP/CLI 协议。
- 代码依赖必须可被自动检查。

## 4. 目标架构

建议把当前 `synapse-core` 内部收敛为以下逻辑层：

- `types`: 请求、响应、错误、指标等纯数据结构。
- `config`: 配置解析、默认值、策略对象。
- `service`: 业务编排与执行决策。
- `runtime`: 进程、文件系统、seccomp、沙箱、平台能力。
- `providers`: 横切能力入口。
- `api`: HTTP 适配。
- `cli`: 命令行装配与启动。

### 4.1 目标依赖方向

```text
Types -> Config -> Service -> Runtime -> UI/API
            ^         ^          ^
            |         |          |
         Providers ---+----------+
```

说明：

- `Providers` 不是业务层，它是横切能力的单入口。
- `UI/API` 只做协议适配，不允许绕过 `Service` 直接调用 `Runtime`。
- `Runtime` 可以依赖 `Providers`，但不能反向依赖 `UI/API`。

### 4.2 模块映射建议

当前实现到目标结构的映射建议如下：

- `types.rs` -> 保留并作为 `types` 层。
- `error.rs` -> 保留并继续作为统一错误层。
- `executor.rs` -> 拆分为 `service` + `runtime`。
- `pool.rs` -> 归入 `service` 或独立为 `sandbox`/`service` 子模块。
- `seccomp.rs` -> 归入 `runtime/linux`。
- `server.rs` -> 保持为 `api` 入口。
- `main.rs` -> 保持为 `cli` 入口，但不再直接做系统探测。

## 5. `Providers` 设计

`Providers` 的职责是收口所有横切关注点。它应该是一个显式、可注入、可替换的接口集合，而不是全局单例。

### 5.1 需要收口的能力

- `auth`: 认证和身份上下文。
- `connectors`: 外部系统连接器。
- `telemetry`: tracing、metrics、事件输出。
- `flags`: 功能标志和发布控制。
- `clock`: 时间源，便于测试。
- `env`: 配置读取，禁止核心层直接 `std::env`。
- `filesystem`: 工作区、临时目录、清理。
- `process`: 子进程启动与管理。
- `platform`: `bwrap`、seccomp、内核能力探测。

### 5.2 设计要求

- `Providers` 必须显式传入，不允许隐式全局访问。
- `Providers` 允许在测试中被替换为假实现。
- 核心层只能依赖抽象接口，不依赖具体 SDK 或进程环境。
- 如果某个能力当前暂时不存在，也可以先留空方法，但接口必须先统一。

### 5.3 建议形态

建议先做成一个轻量 trait + 实现集合，而不是立即做复杂的依赖注入容器。

```text
Providers
  ├── env()
  ├── fs()
  ├── process()
  ├── telemetry()
  ├── flags()
  ├── platform()
  └── auth()
```

## 6. 分阶段改造计划

### 阶段 1：边界收口

目标：先把 ambient 依赖收起来，不改变整体功能。

任务：

1. 新增 `providers` 抽象。
2. 把 `std::env` 读取集中到配置/启动层。
3. 把平台探测、命令查找、临时目录管理收口。
4. 让 `server.rs` 不再直接读运行时配置。
5. 让 `cli` 的 `doctor` 走统一的探测入口。
6. 为测试注入能力提供替身。

交付物：

- `Providers` 定义。
- 配置加载层。
- 现有启动路径不变，但系统访问点更集中。

风险：

- 代码改动较广，但行为应保持等价。
- 主要风险是漏改某个环境读取点。

### 阶段 2：拆分 runtime

目标：把系统执行能力从业务编排里分出去。

任务：

1. 从 `executor.rs` 拆出 runtime 相关逻辑。
2. 保留业务决策在 service 层。
3. 把沙箱目录、脚本写入、子进程启动、seccomp 加载放进 runtime。
4. 让 service 只关心“执行什么、用什么策略执行、结果如何归类”。
5. 保持现有 API 兼容。

交付物：

- `runtime` 模块。
- `service` 模块。
- `executor.rs` 变薄，职责单一。

风险：

- 接口拆分可能带来重复参数，需要控制。
- 需要避免 runtime 反过来依赖 service。

### 阶段 3：收紧 API 与 CLI

目标：让 UI 层只做适配。

任务：

1. `server.rs` 只负责路由、状态注入、HTTP 映射。
2. `main.rs` 只负责命令解析和装配。
3. CLI 的系统检查从业务代码里移除或改为 provider 驱动。
4. 错误映射保持在 API 层，不下沉进 runtime。

交付物：

- 运输层与业务层分离。
- 启动路径更清晰。

风险：

- 如果错误映射逻辑散落，容易造成重复处理。

### 阶段 4：自动化约束

目标：把架构规则变成机器可检查的规则。

任务：

1. 增加模块依赖扫描规则。
2. 禁止 `core` 直接读取 `std::env`、直接使用外部 SDK、直接接触 CLI/HTTP。
3. 禁止 `api` 绕过 service 调 runtime。
4. 禁止横切能力绕过 `Providers`。
5. 增加边界测试和构建检查。

交付物：

- 自动化 lint 或脚本。
- 架构违规时 CI 失败。

风险：

- 规则过严会造成误报，需要分阶段启用。

## 7. 推荐文件落点

建议目标目录保持轻量，不要一次性把项目拆得太碎：

```text
crates/synapse-core/src/
  lib.rs
  types.rs
  error.rs
  config.rs
  providers.rs
  service.rs
  runtime/
    mod.rs
    linux.rs
    portable.rs
  sandbox/
    mod.rs
    pool.rs

crates/synapse-api/src/
  lib.rs
  server.rs
  transport.rs

crates/synapse-cli/src/
  main.rs
  doctor.rs
```

如果短期内不想新增太多文件，也可以先保留现有文件，先把职责边界拆开，再逐步移动。

## 8. 验收标准

重构完成后，应满足以下标准：

1. `types` 层不依赖 runtime、API、CLI。
2. `config` 层只负责配置，不做 IO 执行。
3. `service` 层不直接接触 HTTP 或 CLI。
4. `runtime` 层只负责系统执行，不负责协议适配。
5. `api` 和 `cli` 都通过 `Providers` 访问横切能力。
6. Linux 执行路径仍然要求 `bwrap`，没有静默 fallback。
7. 业务核心不再直接读取环境变量或直接探测命令。
8. 架构依赖规则可以被自动化检查。

## 9. 风险与回滚

### 9.1 主要风险

- 过度抽象：如果过早引入太多接口，会增加阅读成本。
- 接口泄漏：如果 `Providers` 太大，会变成新的“万能对象”。
- 行为漂移：重构时若改变安全边界，会影响执行结果。

### 9.2 回滚策略

- 每一阶段都保持可编译、可测试。
- 每次只移动一类职责，不做跨域大搬迁。
- 先加适配层，再删旧逻辑。
- 行为不变时再推进下一阶段。

## 10. 建议推进顺序

建议按以下节奏推进：

1. 先做阶段 1，收口环境和平台探测。
2. 再做阶段 2，拆 runtime。
3. 再做阶段 3，收紧 API/CLI。
4. 最后做阶段 4，上自动化依赖约束。

## 11. 结论

这次改造是值得做的，但前提是控制尺度：

- 目标是收边界，不是重造框架。
- 目标是让依赖方向可被机器检查，不是只靠代码评审。
- 目标是把横切能力统一收口，不是把核心逻辑包装得更复杂。

对当前项目来说，这是一种务实的结构优化，而不是过度设计。
