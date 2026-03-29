# Synapse 首版产品化执行计划

> 本文档承接 [../product/go-to-market-plan.md](../product/go-to-market-plan.md)，将市场与产品判断转化为可执行的近期工作计划。目标不是继续扩展方向，而是收敛首版、补齐可卖能力、形成 PoC 与交付闭环。

## 0. 当前状态快照（2026-03-29）

以下判断基于仓库内已有代码、脚本、文档与测试结果，不包含仓库外销售活动或客户沟通记录。

### 0.1 已完成或基本完成

- 工作流 A 基本完成：
  - `docs/product/v1-scope.md`
  - `docs/product/not-in-v1.md`
  - `docs/product/ideal-customer-profile.md`
- 工作流 B 已形成首版交付骨架：
  - `docs/api-reference.md`
  - `sdk/python/`
  - `docs/quickstart/enterprise-poc-guide.md`
  - `docs/product/security-whitepaper.md`
  - `docs/product/poc-playbook.md`
  - `examples/pr-review-agent/`
- 工作流 C 已形成首版工程骨架：
  - runtime 导入、激活、验证命令已具备
  - 审计、错误模型、配额与安全测试已存在
  - `scripts/release_gate_v1.sh` 已存在
  - `docs/quickstart/runtime-operations-guide.md` 已补齐运行时获取、验证、更新与损坏处理说明
- 工作流 D 的基础材料已存在：
  - `docs/product/icp-target-list-template.md`
  - `docs/product/sales-messaging.md`
  - `docs/product/poc-playbook.md`
  - `docs/product/pricing-draft.md`
  - `docs/product/demo-script.md`
  - `docs/product/objections-log-template.md`
  - `docs/product/customer-validation-log-template.md`

### 0.2 部分完成，仍需继续执行

- B2 Python SDK 首版已存在，且已补齐基础超时与重试封装；后续仍需持续补齐稳定性与回归覆盖。
- C2 API v1 稳定契约文档与测试已存在，但仍需持续消除实现与门禁之间的遗漏。
- C5 发布门禁已存在，但仍需继续收敛发布参数、阈值和运行环境，确保性能门禁可稳定执行。
- D3 标准演示脚本与 D6 objections 闭环资产已补齐；后续仍需导入真实外部反馈记录。

### 0.3 当前明确未完成或无法仅凭仓库证明已完成

- D1 首批 ICP 名单是否已真实用于外部联系，仓库内仍无法证明。
- D6 是否已完成至少一轮真实客户 objections 记录，仓库内仍暂无访谈或反馈记录。
- 8.4 中“至少完成一轮外部客户访谈或设计合作客户触达”，仓库内暂无可验证证据。

### 0.4 当前执行策略

从 2026-03-29 起，按“一次补一项缺口”的方式推进，每次迭代都必须完成：

1. 一项功能或一项交付闭环修复
2. 对应单元测试、集成测试或 smoke 测试
3. 代码审核，包含安全审核
4. 完成度评估
5. `git add`、`git commit`

## 1. 目标

在未来 8 周内，把 Synapse 从“可用的安全执行后端”推进到“可向设计合作客户交付的企业内 AI 执行平面首版”。

本阶段只服务一个核心目标：

**支撑企业内部 AI 代码助手 / Agent 执行层的首版交付。**

## 2. 范围

### 2.1 In Scope

- 首版产品边界冻结
- API v1 契约冻结
- Python SDK 补齐
- 私有化部署与安全说明补齐
- 审计、错误模型、运行时交付能力增强
- 标准 demo 与 PoC 材料
- 首批 ICP 与客户验证准备

### 2.2 Out of Scope

- 多语言扩展
- 浏览器 / Desktop / Computer Use
- 在线 IDE / 控制台优先建设
- 复杂模板市场与 snapshot 平台
- 公有云托管版
- 面向个人开发者的低价按量模式

## 3. 成功标准

本阶段完成的标准不是“代码更多”，而是满足以下业务与交付条件：

1. 团队内部对首版定位、边界和非目标达成一致。
2. 客户可在 1 天内完成基础部署。
3. 客户可在 1 周内接入一个真实 agent 场景。
4. 安全与平台评估方能理解执行边界、审计和失败语义。
5. 团队具备一套可复用的标准 demo、PoC 方案和销售材料。

## 4. 工作流拆分

本阶段拆成 4 条并行工作流：

1. 产品定义收口
2. 产品化交付补齐
3. 工程能力对齐
4. GTM 与客户验证

## 5. 工作流 A：产品定义收口

### 5.1 目标

把市场计划固化为团队可执行的首版定义，避免工程和 GTM 在中途继续摇摆。

### 5.2 关键任务

#### A1. 冻结首版定位

明确首版定位为：

- 企业内 AI 执行平面
- 面向内部 AI 代码助手 / Agent 执行层
- 私有化部署优先

#### A2. 冻结首版场景

确认首版主场景为：

- 企业内部 AI 代码助手 / Agent 执行层

次级演示场景：

- AI 驱动的 CI/CD 与代码评审

#### A3. 冻结首版边界

明确首版只承诺：

- Python
- 稳定 API
- 审计
- 配额与资源控制
- 私有化部署
- 最薄官方 SDK

#### A4. 明确 Not in v1

形成明确的“本阶段不做”列表，至少包括：

- 多语言
- browser / desktop
- 在线 IDE
- 高级模板平台
- 早期 SaaS 化

### 5.3 交付物

- `docs/product/v1-scope.md`
- `docs/product/not-in-v1.md`
- `docs/product/ideal-customer-profile.md`

### 5.4 验收标准

- 团队在评审会后不再对“首版到底卖什么”产生分歧
- 后续计划和工程任务均能映射到首版边界

## 6. 工作流 B：产品化交付补齐

### 6.1 目标

把现有能力从“工程可用”推进到“客户可接入、销售可讲、PoC 可落地”。

### 6.2 关键任务

#### B1. 冻结 API v1 契约

补齐并冻结以下内容：

- 请求结构
- 响应结构
- 错误码与状态码
- request_id / tenant_id / audit summary
- 超时、内存、CPU 限制字段

#### B2. 发布 Python SDK 首版

首版 SDK 只覆盖：

- `execute`
- `execute_stream`
- token / tenant
- 错误映射
- 基础超时与重试封装

#### B3. 补齐部署文档

至少提供：

- 安装与初始化说明
- runtime 准备路径
- 最小配置说明
- 健康检查与故障排查

#### B4. 补齐安全材料

至少说明：

- 默认安全边界
- 默认拒绝项
- 宿主保护方式
- 审计覆盖范围
- 已知限制

#### B5. 补齐 PoC 接入材料

内容应包括：

- 一个标准 PoC 方案
- 接入步骤
- 成功标准
- 常见失败处理

#### B6. 构建标准 demo

优先做：

- PR Review Agent Demo

可选补充：

- 内部数据分析助手 Demo

### 6.3 交付物

- `docs/api-reference.md` v1 冻结版
- `sdk/python/`
- `docs/quickstart/enterprise-poc-guide.md`
- `docs/product/security-whitepaper.md`
- `examples/pr-review-agent/`

### 6.4 验收标准

- 新客户能在不依赖口头解释的情况下完成最小接入
- 销售与方案团队能基于文档和 demo 独立演示

## 7. 工作流 C：工程能力对齐

### 7.1 目标

围绕首版承诺修正工程优先级，把有限资源用在支撑交付、PoC 和稳定性上。

### 7.2 关键任务

#### C1. 打实 Python runtime 交付能力

降低对宿主 Python 的依赖，明确：

- 默认 runtime 获取方式
- runtime 验证方式
- runtime 更新策略
- runtime 损坏时的错误语义

#### C2. 冻结并清理 API 预览属性

把当前 preview 风格接口收敛成稳定契约，必要时：

- 收敛字段命名
- 收敛错误码
- 收敛 stream 语义
- 增补稳定性测试

#### C3. 强化审计与错误模型

重点补齐：

- timeout / OOM / policy / quota / auth / tenant 失败语义
- 审计字段完整性
- 关键审计路径测试

#### C4. 强化首版运行稳定性

围绕真实 PoC 使用方式验证：

- 并发
- 队列
- 配额
- reset
- runtime 丢失
- audit 持久化异常

#### C5. 建立首版发布门禁

至少覆盖：

- `cargo fmt`
- `cargo clippy`
- `cargo test`
- SDK smoke
- 标准 demo smoke
- 基础性能回归

### 7.3 交付物

- API v1 稳定性测试
- runtime 交付与校验方案
- 错误模型文档与测试矩阵
- `scripts/release_gate_v1.sh`

### 7.4 验收标准

- 团队能稳定跑完首版发布门禁
- 首版演示与 PoC 不依赖临时修补才能跑通

## 8. 工作流 D：GTM 与客户验证

### 8.1 目标

验证首版定位是否真的能推动设计合作客户进入 PoC，而不是停留在内部想象。

### 8.2 关键任务

#### D1. 建立首批 ICP 名单

优先寻找：

- 内部 AI 助手团队
- DevInfra / PR 自动化团队
- 高合规行业的 AI 产品团队

#### D2. 输出标准销售话术

统一三条主价值：

- 不出域
- 可审计
- 不失控

#### D3. 输出标准演示脚本

要求销售、方案、产品、研发都使用同一版本 demo 叙事。

#### D4. 设计 PoC 流程

明确：

- PoC 前提条件
- 部署方式
- 接入工作量
- 验收标准
- 成功与失败判断

#### D5. 输出初版定价草案

当前建议方向：

- 私有化年费许可证
- 按容量档定价
- 附支持服务

#### D6. 记录客户 objections

为后续产品和销售材料迭代建立输入闭环。

### 8.3 交付物

- `docs/product/icp-target-list-template.md`
- `docs/product/sales-messaging.md`
- `docs/product/poc-playbook.md`
- `docs/product/pricing-draft.md`

### 8.4 验收标准

- 至少完成一轮外部客户访谈或设计合作客户触达
- 能复述客户最关心的 5 个问题及当前回答

## 9. 近期优先级

建议按以下顺序推进：

1. 冻结 `v1 scope`
2. 冻结 API v1
3. 补 Python SDK
4. 补部署与安全文档
5. 做 PR Review Agent demo
6. 打实 Python runtime 交付
7. 建首版发布门禁
8. 启动首批设计合作客户验证

## 10. 八周排期建议

### Week 1

- 完成首版定位评审
- 完成 `v1 scope` 与 `not in v1`
- 确认首版主场景与标准 demo

### Week 2

- API v1 字段与错误码冻结
- 明确 SDK 首版范围
- 明确 runtime 首版交付路径

### Week 3

- Python SDK 首版开发
- API v1 文档更新
- 安全白皮书框架完成

### Week 4

- 部署文档与 PoC 接入文档完成初稿
- PR Review Agent demo 初版跑通
- 错误模型与审计测试补强

### Week 5

- runtime 交付能力补齐
- demo 与 SDK 联调
- 销售话术与 demo 脚本初稿完成

### Week 6

- 发布门禁脚本落地
- 首版 smoke 路径固定
- PoC playbook 完成

### Week 7

- 启动首批客户验证
- 收集 objections
- 修正文档、话术与 demo

### Week 8

- 输出首版可交付包
- 形成设计合作客户版本
- 复盘下一阶段需求

## 11. 角色建议

以下为推荐责任划分，具体 owner 可后续补齐：

### 产品

- 维护定位、范围和优先级
- 维护 ICP、场景、PoC 设计
- 统一对外叙事

### 工程

- 落地 API、SDK、runtime、门禁与稳定性
- 保证首版演示和 PoC 可重复运行

### 安全 / 平台

- 审核隔离边界和安全说明
- 审核部署与运维建议

### GTM / 业务

- 目标客户名单
- 销售材料与客户沟通
- PoC 推进与反馈整理

## 12. 风险与控制点

### 12.1 风险：需求回摆

表现：

- 中途重新拉回多语言、browser、IDE、SaaS 等方向

控制：

- 所有新增事项必须经过首版边界评审

### 12.2 风险：文档与材料落后于实现

表现：

- 工程功能有了，但销售与 PoC 无法复用

控制：

- 将文档、SDK、demo 视为里程碑必选项，而不是附属项

### 12.3 风险：PoC 依赖人工救火

表现：

- 每次演示都需要研发临时修问题

控制：

- 建立首版门禁和固定 smoke 路径

## 13. 建议新增文档清单

为支撑本执行计划，建议继续新增以下文档：

- `docs/product/v1-scope.md`
- `docs/product/not-in-v1.md`
- `docs/product/ideal-customer-profile.md`
- `docs/product/security-whitepaper.md`
- `docs/product/sales-messaging.md`
- `docs/product/poc-playbook.md`
- `docs/product/pricing-draft.md`
- `docs/quickstart/enterprise-poc-guide.md`

## 14. 结论

这份执行计划的核心意图只有一个：

**把 Synapse 从“有潜力的安全执行后端”推进到“可以开始被真实客户评估和采购的企业内 AI 执行平面首版”。**

接下来的工作重点不应继续分散到更大的平台愿景，而应集中在：

- 冻结首版
- 补齐交付物
- 提升稳定性
- 做出标准 demo
- 启动客户验证

只有这五件事先成立，后续多语言、控制面、SaaS、复杂模板与高级平台能力才有现实基础。
