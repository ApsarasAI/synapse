# 运维控制台实施计划

## 1. 目标

第一版运维控制台定位为只读运维台，优先解决以下问题：

- 快速判断服务是否异常
- 快速查看最近失败请求和错误分布
- 快速按 `request_id` 与 `tenant_id` 定位单次执行
- 快速区分 runtime、容量、配额、限流、超时等问题类型

第一版不包含高风险写操作，例如重试、取消、切换 runtime、在线调配额。

## 2. 范围

### 必须有

- 服务健康总览
- 执行指标看板
- 错误分类视图
- 请求追踪入口
- 审计时间线
- tenant 维度筛选
- 受控只读权限

### 应该有

- 最近失败请求列表
- runtime 状态面板
- 容量与排队视图
- 输出截断提示
- 多维过滤
- 审计导出
- 基础告警提示

## 3. 验收标准

运维同学应能在 1 分钟内回答以下问题：

- 服务当前是否异常
- 最近哪些请求失败
- 失败的主因是什么
- 问题是否来自 runtime、容量、配额、限流或执行超时
- 指定 `request_id` 对应的审计轨迹是什么

## 4. 实施阶段

### Phase 0: 范围冻结

1. 明确第一版只做只读运维控制台，不包含重试、取消、切换 runtime 等写操作。
2. 冻结两档功能范围。
   必须有：总览、执行指标、错误分类、请求追踪、审计时间线、tenant 筛选、只读鉴权。
   应该有：最近失败请求、runtime 状态、容量/排队视图、输出截断提示、多维过滤、审计导出、基础告警提示。
3. 评审并确认页面数量、接口范围、鉴权边界。

### Phase 1: 后端数据模型

1. 新增 `request summary` 持久化能力。
2. 摘要字段至少包含：
   `request_id`、`tenant_id`、`language`、`status`、`error_code`、`created_at`、`duration_ms`、`queue_wait_ms`、`stdout_truncated`、`stderr_truncated`。
3. 在执行完成路径统一写入摘要记录。
   成功、失败、配额拒绝、限流拒绝都必须落摘要。
4. 保留现有 audit 日志作为单次执行详情来源，摘要存储只负责列表和聚合查询。
5. 第一版优先采用简单稳定的本地存储方案。
   建议优先考虑 `SQLite`；如果实现成本受限，可退而求其次使用 `JSONL + 内存索引`。

### Phase 2: Admin API

1. 实现 `GET /admin/overview`。
   返回健康状态、核心 metrics、top error codes、recent failures。
2. 实现 `GET /admin/requests`。
   支持按 `request_id`、`tenant_id`、`status`、`error_code`、`language`、`from`、`to` 查询。
3. 实现 `GET /admin/requests/:request_id`。
   返回单次请求摘要与执行关键信息。
4. 实现 `GET /admin/requests/:request_id/audit`。
   可复用现有 audit 能力，统一到 admin 路径。
5. 实现 `GET /admin/runtime`。
   返回 active runtime、installed runtimes、verify 状态。
6. 实现 `GET /admin/capacity`。
   返回 admitted、queued、capacity rejected、queue timeout 等容量指标。

### Phase 3: 鉴权与租户隔离

1. 复用现有 bearer auth 中间件保护全部 `/admin/*` 路径。
2. 默认管理员可看全局；非全局角色只能看授权 tenant 数据。
3. 在 `overview`、`requests`、`audit` 查询链路中统一做 tenant 过滤。
4. 审计导出和请求详情接口同样执行 tenant 可见性校验。

### Phase 4: 控制台页面

1. `Dashboard`
   展示健康状态、核心指标卡片、top error codes、recent failures、基础异常提示。
2. `Requests`
   展示查询条件和请求列表，支持多维过滤。
3. `Execution Detail`
   展示请求摘要、错误信息、输出截断标记、审计时间线、原始审计导出入口。
4. `Runtime`
   展示 active runtime、verify 状态、已安装版本、容量与排队视图。

### Phase 5: 必须有功能验收

1. 运维可通过 `request_id` 快速定位到单次请求详情。
2. 最近失败请求必须可从 `Dashboard` 一跳进入详情页。
3. 任意失败请求都必须能定位到错误码和 audit timeline。
4. 任意 tenant 查询结果不得泄露其他 tenant 数据。
5. `Dashboard` 必须能区分以下问题类型：
   `runtime` 问题、容量问题、配额/限流问题、执行超时/资源超限问题。

### Phase 6: 应该有功能补齐

1. 在 overview 聚合接口中直接返回最近失败请求列表。
2. 对接 runtime list / verify 结果，补齐 runtime 状态面板。
3. 用现有 metrics 与 scheduler 信息补齐容量与排队视图。
4. 在 request summary 与 detail 中同时展示输出截断提示。
5. 在 `GET /admin/requests` 中实现多维过滤。
6. 支持下载单次请求的 audit JSON。
7. 前端先用规则判断方式提供基础告警提示，不要求第一版接入外部告警系统。

### Phase 7: 测试与发布

1. 为 admin API 增加集成测试。
   覆盖鉴权、tenant 过滤、列表查询、详情查询、runtime 状态。
2. 为 request summary 存储增加单元测试。
   覆盖成功、失败、拒绝、截断、时间过滤。
3. 执行：
   `cargo fmt --all`
   `cargo clippy --workspace --all-targets -- -D warnings`
   `cargo test --workspace`
4. 补充最小运维文档。
   说明页面用途、接口含义、错误码解释、如何使用 `request_id` 排障。

## 5. 建议排期

1. 第 1 周：摘要存储与 Admin API。
2. 第 2 周：4 个页面、tenant 过滤、基础测试。
3. 第 3 周：补齐 should-have 项、文档、发布验证。

## 6. 当前关键依赖与缺口

当前仓库已经具备以下基础能力：

- `GET /health`
- `GET /metrics`
- `GET /audits/:request_id`
- 执行生命周期 metrics
- request 级 audit 持久化

当前最关键的新增能力不是页面，而是请求摘要索引能力。没有该能力，将无法高效支撑：

- 最近失败请求列表
- 请求列表分页与筛选
- 时间范围查询
- 按 tenant / error_code 聚合

因此，第一版应优先建设 `request summary` 存储与查询能力，再推进控制台页面。
