# Standard Demo Script

## 1. 目标

这份脚本用于统一 Synapse v1 的标准对外演示口径，供销售、方案、客户成功和研发复用。

默认演示场景：

- PR Review Agent

## 2. 演示对象

优先面向以下角色：

- AI 平台负责人
- DevInfra / CI 自动化负责人
- 安全与平台评估方

## 3. 开场话术

建议先用一句话说明：

Synapse 是面向企业 AI 系统的私有化执行平面，让模型能执行代码，但不越过企业的安全边界。

然后明确三条主价值：

- 不出域
- 可审计
- 不失控

## 4. 演示前准备

演示前确认：

- `cargo run -p synapse-cli -- doctor` 可通过关键检查
- 服务已启动
- Python runtime 已导入并激活
- `examples/pr-review-agent/run_demo.py` 可在演示环境运行

最小检查路径：

```bash
cargo run -p synapse-cli -- runtime import-host --language python --version system --command python3 --activate
cargo run -p synapse-cli -- serve --listen 127.0.0.1:8080
python examples/pr-review-agent/run_demo.py
```

## 5. 演示流程

### 步骤一：定义客户问题

先说明客户当前问题不是“模型不会写代码”，而是：

- 执行发生在不可控环境
- 无法解释失败原因
- 缺少租户与审计边界

### 步骤二：展示最小执行闭环

展示 Synapse 可以接收请求、执行 Python 代码并返回结构化结果。

强调字段：

- `request_id`
- `tenant_id`
- `audit`
- `limits`

### 步骤三：展示标准 demo

运行：

```bash
python examples/pr-review-agent/run_demo.py
```

讲解顺序：

- 输入是一个待评审 diff
- 评审逻辑封装为 Python 脚本
- 脚本通过 Synapse 执行
- 输出 findings，可复用于 PR Review Agent 工作流

### 步骤四：展示安全与失败语义

明确说明：

- 网络默认受限
- 资源限制内建于接口
- 错误码可直接进入告警和审计链路

重点可以点名：

- `sandbox_policy_blocked`
- `wall_timeout`
- `tenant_forbidden`

### 步骤五：收口到客户 PoC

最后把演示收口到 PoC，而不是泛化平台愿景。

建议结尾：

- 先跑通一个真实 agent 场景
- 用同一套 SDK、文档和 demo 联调
- 用 objections 模板记录阻塞点

## 6. 成功标准

一次成功演示至少应满足：

- 听众能复述 Synapse 的产品定位
- 听众理解默认安全边界
- 听众理解标准接入路径
- 听众愿意进入 PoC 或继续技术评估

## 7. 演示后记录

每次演示后至少记录：

- 客户团队与场景
- 最关心的 3 个问题
- 当前 objections
- 下一步动作

建议使用：

- `docs/product/objections-log-template.md`
