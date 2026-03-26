# Synapse Enterprise Sandbox Roadmap

本文档定义 Synapse 面向企业级安全稳定产品的渐进式研发路线，遵循“先做可信执行内核，再做企业级平台能力”的原则。

## 1. 总体目标

### Phase 0: 建立可信内核

目标：先把“能安全执行”做扎实，而不是先做很多平台功能。

验收标准：

- 单语言 Python 执行链路稳定
- 明确的隔离边界和失败语义
- 压测下无明显资源泄漏、僵尸进程、目录残留
- 安全测试和回归测试可重复运行

### Phase 1: 做成可运营产品

目标：从“可运行”升级到“可上线”。

验收标准：

- 有配额、排队、容量保护、审计、指标
- 有稳定 API 契约
- 有部署前检查和故障定位能力
- 可以支持企业 PoC

### Phase 2: 做成企业级平台

目标：支持多租户、多运行时、版本治理和合规能力。

验收标准：

- 运行时可版本化管理
- 多租户公平调度
- 企业审计、策略和运维能力完善
- 可支撑正式生产环境

## 2. 分阶段路线图

## Phase 0：0 到 6 周

主题：先把执行内核做硬。

### P0.1 沙箱文件系统重构

当前问题：

- 之前只是临时目录加 `bwrap` 绑定，不是真正的 runtime layer
- 池化收益有限，重置方式也偏粗糙

目标：

- 引入 OverlayFS
- runtime 只读层共享
- 每次执行创建独立 upper/work 层
- 执行结束彻底销毁写层

交付物：

- runtime layer 目录布局设计
- OverlayFS 挂载实现
- 生命周期清理逻辑
- 对应集成测试

验收标准：

- 无法读取宿主敏感文件
- 每次执行的文件改动不会污染下一次执行
- 连续高频执行后无残留 upper/work 目录

当前进展（2026-03-26）：

- Linux 执行链路现在通过 `bubblewrap` overlay 参数把 `/workspace` 挂成共享 lower layer + 每沙箱 `upper/work` 写层
- 池化沙箱的 reset 已切换为重建写层目录，执行后的 `/workspace` 改动不会带到下一次运行
- 已增加集成测试验证跨运行写隔离和 reset 后仅保留 `upper/work` 两类目录

### P0.2 Runtime 供应链治理

当前问题：

- 仍依赖宿主机 `python3`
- 没有版本、完整性、回滚能力

目标：

- 不再直接依赖宿主 PATH
- 引入 runtime manifest
- 支持 runtime 安装、激活、校验

交付物：

- runtime manifest 结构
- runtime store 目录规范
- CLI：`runtime list/verify/install/import-host/activate`
- hash 校验与加载逻辑

验收标准：

- 明确知道执行使用的是哪个 runtime 版本
- runtime 丢失或损坏时错误清晰
- 可回滚到前一版本

当前进展（2026-03-26）：

- 已增加 managed runtime store，包含 `runtimes/<language>/<version>/manifest.json` 和 active version 指针
- 已支持 `synapse runtime list/verify/install/install-bundle/import-host/activate`
- runtime 解析已加入 SHA-256 完整性校验，损坏或缺失时会返回明确 `runtime_unavailable`
- 当前仍仅支持 Python；虽然现在可从离线 runtime bundle 安装受控工件，但显式 `import-host` 工作流仍会从宿主 `python3` 导入 `python:system`，距离完全独立 runtime 供应链还有差距

### P0.3 容量控制与执行调度

当前问题：

- 池耗尽直接 overflow
- 只有租户级 semaphore，没有全局调度

当前进展（2026-03-26）：

- 已落地全局 execution scheduler，执行入口不再在池耗尽时无限制 overflow
- 已支持队列深度限制、排队超时、容量拒绝和对应 metrics / API 错误码
- 已补 contention 测试和租户轮转公平性测试

目标：

- 增加全局执行队列
- 支持排队超时
- 支持最大并发与背压
- 明确拒绝策略

交付物：

- 全局 scheduler
- queue metrics
- 排队和拒绝错误码
- contention 测试

验收标准：

- 高并发下系统不失控
- 不出现无限 overflow 创建
- 请求在排队、执行、拒绝之间状态清晰

### P0.4 安全基线测试

目标：

- 建立企业级产品最重要的回归资产

交付物：

- 文件系统逃逸测试
- fork/clone/exec/network 测试
- OOM/CPU timeout 测试
- 并发与资源泄漏测试

验收标准：

- 每次发布前自动执行
- 关键安全回归可复现

当前进展（2026-03-26）：

- 已有文件系统隔离、环境变量隔离、fork 阻断、timeout、OOM 和容量争用相关测试
- 已补 network 尝试审计测试、进程创建审计测试、池化 reset 隔离测试，以及单入口 `scripts/p0_gate.sh` 发布门禁
- 资源泄漏基线目前仍主要通过池化/重置测试间接覆盖，后续可继续补更强的 mount/zombie 专项探针

## Phase 1：6 到 12 周

主题：做成可上线的服务。

### P1.1 审计体系升级

当前问题：

- 现在更多是请求生命周期事件，不是安全审计事件

目标：

- 记录真正对企业有价值的审计信息

优先审计项：

- 请求 ID、租户 ID、runtime 版本
- 配额命中、限流、排队超时
- seccomp 命中
- OOM、CPU 超限、wall timeout
- 网络尝试、子进程创建尝试

交付物：

- 审计事件模型 v1
- 审计持久化策略
- `/audits/:request_id` 契约稳定化

验收标准：

- 能回答“这次执行做了什么、为什么失败、命中了什么策略”

### P1.2 错误模型与 API 契约冻结

目标：

- 让 API 能被 SDK、平台和企业系统稳定集成

补齐错误类别：

- `invalid_input`
- `runtime_unavailable`
- `queue_timeout`
- `pool_exhausted` 或 `capacity_rejected`
- `wall_timeout`
- `cpu_limit_exceeded`
- `memory_limit_exceeded`
- `sandbox_policy_blocked`
- `audit_failed`
- `internal_error`

交付物：

- 错误码表
- API schema 文档
- 覆盖所有错误路径的测试

验收标准：

- 同类错误表现一致
- 错误码可机读，message 可读

### P1.3 真流式执行

当前问题：

- 现在 stream 接口本质不是实时流

目标：

- 实时增量输出 stdout/stderr
- 支持 `started/progress/completed/error` 生命周期事件

交付物：

- runtime 输出流式转发
- websocket 生命周期管理
- stream 集成测试

验收标准：

- 长任务执行时客户端可持续收到输出
- 中断、断连、错误路径行为清晰

### P1.4 运维与诊断

目标：

- 出问题时能快速定位

交付物：

- `doctor` 增强
- metrics 扩展
- 健康检查分级：`liveness/readiness/dependency`
- 最小运维手册

核心指标：

- 执行总数、成功率、失败率
- 队列长度、排队时间
- timeout/OOM/policy block 次数
- pool 命中率
- runtime 加载失败次数

## Phase 2：3 到 6 个月

主题：企业级平台化。

### P2.1 多运行时与版本治理

目标：

- Python 之外扩展 Node.js、Shell 等
- 每个 runtime 可版本化、可审计、可回滚

交付物：

- 通用 runtime registry
- runtime compatibility matrix
- runtime policy 配置

### P2.2 多租户公平调度

目标：

- 真正解决 SaaS 和企业共享集群问题

交付物：

- tenant queue
- fairness scheduler
- 权重、优先级、保底额度
- starvation prevention 测试

### P2.3 企业策略能力

目标：

- 让安全团队可控

交付物：

- 租户级资源策略
- 网络策略
- runtime 白名单
- 审计保留期与脱敏策略

### P2.4 部署与高可用

目标：

- 从单机产品走向生产系统

交付物：

- 单机生产部署方案
- 多实例无状态部署方案
- 外部审计存储和指标接入
- 灰度与回滚流程

## 3. 当前工作重点

如果只保留 4 个近期重点，建议如下：

1. OverlayFS 与 runtime layer 落地
2. runtime manifest 和版本治理
3. 全局队列、背压和拒绝策略
4. 安全回归测试体系

这四项完成后，产品才算真正进入“企业级基础盘搭好了”的状态。

## 4. 明确暂缓的事项

这些不是没价值，而是当前阶段应降优先级：

- 过早扩多语言
- 过早做 SDK
- 过早做复杂自适应 seccomp
- 过早做企业管理台
- 过早做华丽 streaming 交互层

原则：底座没做硬，外围能力越多，后续返工越大。

## 5. 建议的里程碑

### M1

时间：第 2 周

结果：

- 确认隔离边界
- 完成 runtime layer 技术方案
- 完成 scheduler 设计

### M2

时间：第 6 周

结果：

- OverlayFS 和 runtime store 可用
- 全局调度与背压上线
- 安全回归测试可跑

### M3

时间：第 10 周

结果：

- 审计 v1、错误模型 v1、metrics v1 稳定
- 支持 PoC 级企业接入

### M4

时间：第 16 周

结果：

- 多租户公平调度
- 多 runtime 初版
- 企业策略能力初版

## 6. 项目管理建议

研发组织建议分成 3 条主线并行：

- `Sandbox Kernel`
  负责人关注：隔离、文件系统、seccomp、cgroup、runtime
- `Control Plane`
  负责人关注：调度、配额、错误模型、审计、配置
- `Service Interface`
  负责人关注：API、stream、CLI、文档、SDK

这样可以避免把所有事情都堆在执行引擎里。
