# Objections Log Template

## 1. 目标

这份模板用于记录设计合作客户、PoC 客户和销售推进中的常见 objections，形成 GTM 与产品迭代的闭环输入。

## 2. 使用方式

每次外部演示、技术评估或 PoC 复盘后，都应新增一条记录。

推荐按周汇总，避免 objections 只停留在聊天记录或口头记忆里。

## 3. 记录字段

| 字段 | 说明 |
| --- | --- |
| Date | 记录日期 |
| Account | 客户或团队名称 |
| Stage | `intro`, `qualified`, `demo`, `poc`, `security_review`, `closed_lost`, `closed_won` |
| Use Case | 当前验证场景 |
| Objection | 客户提出的问题或阻塞 |
| Category | `security`, `deployment`, `runtime`, `pricing`, `product_scope`, `integration`, `performance`, `other` |
| Severity | `low`, `medium`, `high`, `blocking` |
| Current Answer | 当前统一回答口径 |
| Artifact Gap | 当前缺少的文档、测试、功能或材料 |
| Owner | 下一步 owner |
| Next Step | 下一步动作 |
| Status | `open`, `monitoring`, `resolved`, `deferred` |

## 4. 高频 objections 分类

建议重点观察以下类型：

- 安全边界是否足够清晰
- 私有化部署是否足够简单
- runtime 供应链是否可信
- API / SDK 接入成本是否过高
- v1 边界是否与客户预期不一致
- 定价与支持模式是否可接受

## 5. 周度复盘输出

每周至少汇总：

- Top 5 objections
- 本周新增 blocking 项
- 已关闭 objections
- 需要新增的文档或测试
- 需要进入产品路线的缺口

## 6. 示例

| Date | Account | Stage | Use Case | Objection | Category | Severity | Current Answer | Artifact Gap | Owner | Next Step | Status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-03-29 | Example Bank | demo | PR review agent | 现有版本是否支持网络白名单放通 | security | high | v1 默认不支持 allow-list 网络放通，当前聚焦无外网执行与审计闭环 | 需要更明确的安全白皮书 FAQ | product | 演示后补充 FAQ 并确认是否仍进入 PoC | open |
