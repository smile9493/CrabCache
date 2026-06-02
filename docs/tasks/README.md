# 供应商扩展任务清单

> **已迁移至 Trellis 任务系统** — 以下任务现在由 `.trellis/tasks/` 管理。

## Trellis 任务路径

| 任务 | Trellis 路径 | 优先级 | 状态 |
|------|-------------|--------|------|
| 项目总纲 | `.trellis/tasks/06-02-provider-expansion/` | P0 | planning |
| Phase 1a: 核心枚举扩展 | `.trellis/tasks/06-02-core-enums/` | P0 | planning |
| Phase 1b: Profile 路由扩展 | `.trellis/tasks/06-02-profile-routing/` | P0 | planning |
| Phase 1c: Pipeline 选择更新 | `.trellis/tasks/06-02-pipeline-select/` | P0 | planning |
| Phase 2: 配置模板 | `.trellis/tasks/06-02-config-templates/` | P1 | planning |
| Phase 3: Dashboard UI | `.trellis/tasks/06-02-dashboard-ui/` | P2 | planning |
| Phase 4: 测试验证 | `.trellis/tasks/06-02-testing/` | P1 | planning |
| Phase 5: 文档更新 | `.trellis/tasks/06-02-docs-update/` | P2 | planning |

## 使用方式

```bash
# 查看所有任务
python3 ./.trellis/scripts/task.py list

# 启动某个子任务（进入实现阶段）
python3 ./.trellis/scripts/task.py start 06-02-core-enums

# 查看当前激活任务
python3 ./.trellis/scripts/task.py current --source
```

## 相关文档

- [项目任务总纲](../PROVIDER_EXPANSION_PLAN.md)
- [OmniRoute 供应商参考](../OMNIRROUTE_PROVIDERS_REFERENCE.md)
