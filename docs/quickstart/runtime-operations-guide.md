# Runtime Operations Guide

## 1. 目标

这份文档说明 Synapse v1 中 Python runtime 的获取、验证、更新与故障处理方式，作为私有化部署和首版 PoC 的标准运维说明。

## 2. 默认获取方式

v1 当前支持两种主要路径：

### 方式一：导入宿主 Python

适用于快速 PoC 或演示环境。

```bash
cargo run -p synapse-cli -- runtime import-host --language python --version system --command python3 --activate
```

特点：

- 上手最快
- 依赖宿主 `python3`
- 适合首次部署验证

### 方式二：安装离线 bundle

适用于需要更强一致性的环境。

```bash
cargo run -p synapse-cli -- runtime install-bundle --source /path/to/runtime-bundle --activate
```

特点：

- runtime 工件可控
- 更适合正式 PoC 和重复部署
- 更便于版本治理

## 3. 验证方式

完成导入或安装后，必须执行：

```bash
cargo run -p synapse-cli -- runtime verify --language python
```

验证通过时，应能看到：

- `verified`
- `python`
- 当前激活版本
- 对应 binary 路径

如需查看所有安装版本，执行：

```bash
cargo run -p synapse-cli -- runtime list
```

其中健康状态会显示为：

- `ok`
- `corrupt`

## 4. 更新策略

v1 建议采用“新版本安装后显式激活”的方式，而不是原地覆盖。

推荐步骤：

1. 安装新的 host import 或 bundle 版本
2. 执行 `runtime verify`
3. 显式 `activate`
4. 执行 quickstart smoke、SDK smoke 和标准 demo smoke
5. 再切换生产或 PoC 环境流量

推荐命令：

```bash
cargo run -p synapse-cli -- runtime activate --language python --version 3.12.6
```

## 5. 损坏时的错误语义

如果激活 runtime 缺失、损坏或校验失败，执行路径应表现为：

- `runtime verify` 失败
- 服务执行请求返回 `runtime_unavailable`
- 需要重新导入、重新安装或重新激活 runtime

这类故障不应被解释为用户代码错误。

## 6. 故障处理建议

常见处理顺序：

1. 运行 `runtime list`
2. 运行 `runtime verify --language python`
3. 如果是 host import 版本，重新执行 `import-host --activate`
4. 如果是 bundle 版本，重新执行 `install-bundle --activate`
5. 重新运行发布门禁或最小 smoke 路径

## 7. 发布前检查

在发布或 PoC 演示前，至少确认：

- 已有一个 active Python runtime
- `runtime verify` 可通过
- `scripts/release_gate_v1.sh` 可通过
- 标准 demo 可重复执行

## 8. 与其他材料的关系

配合以下文档一起使用：

- `docs/quickstart/enterprise-poc-guide.md`
- `docs/api-reference.md`
- `docs/product/security-whitepaper.md`
