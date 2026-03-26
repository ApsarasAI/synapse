# Docs Index

`docs/` 目录按文档用途组织，避免产品、架构、规划和临时记录混放。

## 目录结构

- `architecture/`
  - 架构设计、技术方案、底层实现分析
- `product/`
  - 产品需求和产品视角文档
- `plans/`
  - 改造计划、研发排期、阶段性待办
- `roadmaps/`
  - 中长期路线图和里程碑
- `quickstart/`
  - 对外开发者上手文档和 smoke 路径
- `CODEMAPS/`
  - 代码结构导图和依赖说明
- `session-snapshots/`
  - 会话快照和临时工作记录

## 当前主要文档

- `product/product.md`
  - 产品需求文档
- `architecture/tech-design.md`
  - 技术设计文档
- `architecture/overlayfs-solution-analysis.md`
  - OverlayFS 方案分析
- `plans/architecture-refactor-plan.md`
  - 架构改造计划
- `plans/pmo.md`
  - 研发排期
- `plans/todo.md`
  - 当前待办和完成度评估
- `roadmaps/enterprise-sandbox-roadmap.md`
  - 企业级 sandbox 渐进式路线图
- `quickstart/10-minute-quickstart.md`
  - 10 分钟跑通 doctor / serve / execute / audits / metrics
- `api-reference.md`
  - 对外 HTTP 接口与错误码示例
- `release-process.md`
  - GitHub Releases 分发与校验流程

## 约定

- 新的产品文档放入 `product/`
- 新的架构或实现分析文档放入 `architecture/`
- 迭代计划、执行计划、待办清单放入 `plans/`
- 版本路线和里程碑规划放入 `roadmaps/`
- 临时会话记录仅放入 `session-snapshots/`
