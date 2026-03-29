# PR Review Agent Demo

这个 demo 用最小方式演示如何把 Synapse 接入到一个 PR Review Agent 工作流中。

## 目标

- 读取一段待评审代码
- 把评审逻辑封装为 Python 脚本
- 通过 Synapse 执行并返回结果

## 前提

- Synapse API 已启动
- Python runtime 已可用
- Python 3.10+ 环境可用

从仓库根目录直接运行时，demo 会自动加载 `sdk/python/src`，不需要额外执行 `pip install -e sdk/python`。

## 运行

```bash
python examples/pr-review-agent/run_demo.py
```

可选环境变量：

- `SYNAPSE_BASE_URL`
- `SYNAPSE_TOKEN`
- `SYNAPSE_TENANT_ID`
- `SYNAPSE_REQUEST_ID`

## 期望输出

脚本会提交一段模拟 diff 分析任务，并返回一个简单的 review 结果，证明：

- SDK 可以连通 Synapse
- Synapse 可以执行 Python 代码
- `request_id` 可用于审计链路追踪
