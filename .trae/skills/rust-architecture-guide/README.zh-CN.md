# Rust 架构指南

**通用 Rust 工程决策宪法** — 覆盖架构设计、地道编码风格、元编程、FFI 互操作、性能调优与高阶质量保证，适用于所有 Rust 项目。

[English](README.md) | 简体中文

## 概述

本指南是 AI 编码助手的**宪法性基础**，提供：

- **四级优先级体系**（P0 安全 → P1 可维护 → P2 编译时 → P3 性能）
- **三种执行模式**（rapid / standard / strict）
- **类型驱动架构**（状态机、Newtype、零成本抽象边界）
- **所有权分层策略**（业务层 Owned，热点层零拷贝）
- **错误处理分层**（库级 `thiserror`，应用级 `anyhow`）
- **截拳道编码风格**（截击样板、经济法则、硬件同理心）
- **Agent 自检清单** + Decision Summary 输出契约

## 核心哲学

> **系统边界与热点路径追求极致，内部流程与冷路径释放心智负担。**

### 优先级金字塔

| 优先级 | 焦点 | 规则 |
|--------|------|------|
| **P0** | 安全与正确性 | 内存安全、数据一致性 — 不可妥协 |
| **P1** | 可维护性 | 可读性、局部复杂度控制 — 默认追求 |
| **P2** | 编译时 | 构建速度、CI/CD 效率 — 测量后决策 |
| **P3** | 运行时性能 | 仅限已证明的瓶颈 — 需 Profiler 证据 |

### 唯物辩证法

工程决策通过矛盾解决：
- **对立统一**：`unsafe` 与 safe 不是敌人——`unsafe` 是安全抽象的物质基础
- **量变到质变**：MVP 的 `Option<bool>` 标志积累后，必须质变为 Enum 状态机
- **否定之否定**：错误不是终点——panic → catch → 优雅降级，每一次否定都走向更高层的韧性

### 截拳道编码哲学

| 原则 | 描述 |
|------|------|
| **截击样板** | 如果一个逻辑可以用 1 行模式匹配表达，绝不使用 5 行嵌套 |
| **经济法则** | 每一行代码都应直接指向意图。消除多余的中间变量与隐式拷贝 |
| **硬件同理心** | 利用迭代器和零拷贝类型，顺应编译器的内联优化 |

### 执行模式

| 模式 | 强制执行 | 权衡 |
|------|---------|------|
| `rapid` | 仅 P0 | 无限 `.clone()`、库中用 `anyhow`、无文档测试 |
| `standard` | P0 + P1 | 大多数项目的默认选择 |
| `strict` | P0–P3 | 所有偏差需要正式的 `// DEVIATION:` 注解 |

## 文档索引

### 执行与策略（7 份）

| 文件 | 主题 |
|------|------|
| `references/00-mode-guide.md` | 执行模式 — rapid / standard / strict 定义与转换 |
| `references/01-priority-pyramid.md` | 四级优先级金字塔 |
| `references/02-conflict-resolution.md` | 典型冲突与解决方案 |
| `references/03-progressive-architecture.md` | MVP → 生产级渐进式架构 |
| `references/04-trade-offs.md` | 权衡决策分析框架 |
| `references/05-glossary.md` | 集中术语词汇表 |
| `references/06-deviation-process.md` | 偏差流程（`// DEVIATION:` 注解） |

### 架构模式（14 份）

| 文件 | 主题 |
|------|------|
| `references/07-state-machine.md` | 类型驱动状态机设计 |
| `references/08-newtype.md` | Newtype 模式与类型安全 ID |
| `references/09-data-architecture.md` | 所有权、克隆、内存布局 |
| `references/10-error-handling.md` | 库级 `thiserror` vs 应用级 `anyhow` |
| `references/11-concurrency.md` | 并发：通道、锁、RwLock、parking_lot、死锁预防 |
| `references/12-async-internals.md` | 异步内幕：Future、Pin/Unpin、select!/join!、取消安全 |
| `references/13-api-design.md` | 公共 API：`#[non_exhaustive]`、Sealed Trait、`#[deprecated]` |
| `references/14-metaprogramming.md` | 截击样板：声明式宏、过程宏、const fn、常量泛型 |
| `references/15-ffi-interop.md` | 防波堤体系：三层隔离、不透明指针、Panic 截击、repr(C) |
| `references/16-observability.md` | Tracing、Metrics、Panic Hook、Coredump |
| `references/17-toolchain.md` | CI、Clippy、rustfmt、Workspace、Feature Flags、cargo deny |
| `references/30-memory-layout.md` | **内存布局透明化**：结构体空隙审计、repr(C) 强制、缓存行友好设计、对齐控制 |
| `references/31-breakwater-pattern.md` | **防波堤架构**：Facade/Core 分层模式、边界截击协议、类型收缩（去氧） |
| `references/32-physical-audit.md` | **物理可行性审计**：I/O 预算、内存天花板、并发真实代价、设计前强制审计 |

### 地道风格（7 份）

| 文件 | 主题 |
|------|------|
| `references/18-control-flow.md` | `let else`、`matches!`、截击深度嵌套 |
| `references/19-iterators.md` | 迭代器链、`filter_map`、流式发力 |
| `references/20-traits.md` | `From` vs `Into`、`Default`、硬件同理心 |
| `references/21-errors.md` | `unwrap_or_else`、`map_err`、`and_then` |
| `references/22-data-struct.md` | 字段简写、类型口吃消除 |
| `references/23-borrowing.md` | `AsRef`、`Cow`、内存经济 |
| `references/24-refactor.md` | Agent 自检清单、归约指令 |

### 性能与质量（4 份）

| 文件 | 主题 |
|------|------|
| `references/25-performance-tuning.md` | 硬件同理心：内存、缓存、无锁、SIMD、BCE、预取 |
| `references/26-advanced-testing.md` | 机器对抗机器：proptest、fuzzing、loom、Miri、turmoil、防御报告 |
| `references/27-review.md` | 综合审查清单 |
| `references/28-usage-examples.md` | 实战用例 |

## 关系

- **独立使用**：适用于所有 Rust 项目（Web 服务、CLI 工具、库等）
- **组合使用**：`rust-systems-cloud-infra-guide` 提供云基础设施场景的垂直深化

## 许可证

MIT
