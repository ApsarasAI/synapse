<!-- Generated: 2026-03-25 | Files scanned: 24 | Token estimate: ~180 -->
# 前端架构

## 页面树
- 无 Web 前端页面树
- 该仓库当前只包含 CLI 和 HTTP API

## 组件层级
- 无 React/Vue/Svelte 组件
- 无静态资源目录

## 状态管理流
```
CLI args
  -> rust enum Commands
  -> serve / doctor / runtime

HTTP state
  -> AppState
  -> SandboxPool
  -> in-memory metrics
```

## 结论
- 当前代码库没有前端实现
- 如果后续加入控制台或仪表盘，建议单独放在新 crate 或独立目录
