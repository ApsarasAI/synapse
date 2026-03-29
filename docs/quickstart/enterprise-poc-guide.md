# Enterprise PoC Guide

## 1. 目标

本指南用于帮助设计合作客户在 1 天内完成 Synapse v1 的基础部署，并在 1 周内接入一个真实 agent 场景。

## 2. 部署前检查

宿主要求：

- Linux
- `bwrap`
- `strace`
- cgroup v2
- `python3`

先执行：

```bash
cargo run -p synapse-cli -- doctor
```

## 3. 准备 Python Runtime

导入宿主 Python 并激活：

```bash
cargo run -p synapse-cli -- runtime import-host --language python --version system --command python3 --activate
```

验证：

```bash
cargo run -p synapse-cli -- runtime verify --language python
```

更完整的获取、更新与损坏处理说明见：

- `docs/quickstart/runtime-operations-guide.md`

## 4. 启动服务

```bash
cargo run -p synapse-cli -- serve --listen 127.0.0.1:8080
```

如果需要鉴权，配置：

```bash
export SYNAPSE_API_TOKENS='[
  {"token":"poc-token","tenants":["default"]}
]'
```

## 5. 验证最小路径

健康检查：

```bash
curl http://127.0.0.1:8080/health
```

执行请求：

```bash
curl \
  -X POST http://127.0.0.1:8080/execute \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer poc-token' \
  -H 'x-synapse-request-id: enterprise-poc-demo' \
  -d '{
    "language": "python",
    "code": "print(\"enterprise poc ok\")\n",
    "timeout_ms": 5000,
    "memory_limit_mb": 128
  }'
```

校验：

- `stdout` 返回预期结果
- `request_id` 与 `tenant_id` 存在
- `audit.event_count` 存在

## 6. 使用 Python SDK 接入

安装本地 SDK：

```bash
pip install -e sdk/python
```

如果只是从仓库根目录运行标准 demo，可以直接运行 `examples/pr-review-agent/run_demo.py`；该脚本会自动加载 `sdk/python/src`。

最小示例：

```python
from synapse_sdk import SynapseClient, SynapseClientConfig

client = SynapseClient(
    SynapseClientConfig(
        base_url="http://127.0.0.1:8080",
        token="poc-token",
        tenant_id="default",
    )
)

response = client.execute(
    "print('sdk ok')\n",
    request_id="sdk-poc-demo",
)
print(response["stdout"])
```

## 7. 推荐首个场景

默认推荐跑通：

- `examples/pr-review-agent/`

这条路径可用于销售演示、方案联调与客户 PoC 首次验证。

## 8. 验收标准

- 服务能稳定通过 `/health`、`/execute`、`/metrics`
- Python SDK 能跑通最小示例
- 标准 demo 可重复运行
- 客户能明确成功或失败判定

## 9. 故障排查

`runtime_unavailable`

- 重新执行 runtime 导入与验证

`sandbox_policy_blocked`

- 当前版本不支持 `allow_list` 网络策略

`auth_required` 或 `auth_invalid`

- 检查 `SYNAPSE_API_TOKENS` 与请求头
