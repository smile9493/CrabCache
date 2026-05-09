# Rust → Wasm 垂直基建编译与边界规范

[![Version](https://img.shields.io/badge/Version-v4.0.0-purple.svg)]()
[![Reference Docs](https://img.shields.io/badge/Reference-13%20Docs-blue.svg)]()
[![Domain](https://img.shields.io/badge/Domain-Wasm%20Vertical%20Base-9b59b6.svg)]()

**Rust-Wasm 前端基建垂直深化架构规范** — 针对 `wasm32-unknown-unknown` 目标的编译配置、跨语言边界、线性内存管理、并发模型到通用代码适配的硬约束规范。

[English](README.md) | 简体中文

---

## 概述

本规范继承 [`rust-architecture-guide`](../rust-architecture-guide/) 的核心哲学，针对 `wasm32-unknown-unknown` 目标的特殊性（线性内存、单线程事件循环、跨语言边界）进行垂直深化。

本 Skill 是所有 Rust+Wasm 前端架构（包括无 DOM 渲染引擎）的编译与边界层基础，必须被继承和遵守。

**环境假设**：Wasm 线性内存只增不减、JS ↔ Wasm 边界是昂贵的 RPC、浏览器主线程不容阻塞、零拷贝视图是 unsafe 操作。

## 核心哲学

本规范是"唯物辩证主义与截拳道"架构哲学在编译与边界层的工程化落地。它基于以下物理事实：

- **Wasm 线性内存只增不减**，必须内部回收。
- **JS ↔ Wasm 边界是昂贵的 RPC**，任何隐式转换都是性能负债。
- **浏览器主线程不容阻塞**，必须顺应事件循环。
- **零拷贝视图是 unsafe 操作**，生命周期必须受规约约束。

由此衍生四条铁律：

| ID | 铁律 | 含义 |
|----|------|------|
| **IRON-01** | 体积即王道 | 编译产物必须被极致压缩，所有调试信息、展开表、冗余段全部剥离 |
| **IRON-02** | 边界零拷贝 | 高频数据路径上，永远传递指针与长度，绝不进行序列化；零拷贝视图的生命周期必须在文档中标注 |
| **IRON-03** | 内存分治 | 全局状态静态驻留，帧级对象随帧湮灭，杜绝动态分配碎片 |
| **IRON-04** | 跨源隔离可见即所得 | 凡启用 `SharedArrayBuffer` 必须文档化 COOP/COEP 头配置要求 |

---

## 文档索引

`references/` 目录包含 **12 份参考文档**，严格对应规范的 12 个阶段：

### 一、核心哲学与铁律

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **01** | [iron-rules.md](references/01-iron-rules.md) | 四条铁律 IRON-01~04 及其物理依据，铁律之间的关系 |

### 二、编译与产物控制

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **02** | [build-control.md](references/02-build-control.md) | `Cargo.toml` 发布配置（MUST）+ `wasm-opt` 后处理优化（MUST）+ 分配器替换选型（SHOULD） |

### 三、FFI 与跨语言边界

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **03** | [ffi-boundary.md](references/03-ffi-boundary.md) | `WasmSlice` 零拷贝安全封装模板（MUST）+ 边界安全风险 + 边界类型显式契约（MUST） |

### 四、内存与生命周期

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **04** | [memory-lifecycle.md](references/04-memory-lifecycle.md) | 生命周期分离：全局驻留 vs 帧级生灭（MUST）+ 内存泄漏的物理防御（MUST） |

### 五、并发与事件驱动

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **05** | [concurrency-events.md](references/05-concurrency-events.md) | 异步模型映射（MUST）+ 阻塞型 API 禁令（MUST NOT）+ Worker 隔离与 SharedArrayBuffer（SHOULD） |

### 六、通用代码规范的 Wasm 适配

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **06** | [wasm-adaptation.md](references/06-wasm-adaptation.md) | 错误处理 `Result<T, JsValue>` + `thiserror`（MUST）+ 日志 `console_error_panic_hook` + `tracing_wasm`（MUST） |

### 七、禁止清单与合规自检

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **07** | [prohibitions-checklist.md](references/07-prohibitions-checklist.md) | 7 条硬禁令 [F-01]~[F-07] + 10 项合规自检清单 |

### 八、架构哲学与决策元规范 V2.0

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **08** | [philosophy-v2.md](references/08-philosophy-v2.md) | 唯物辩证主义核心信条 + 截拳道虚实相生 + 分级防御策略 + 架构判准决策树 + 代码审查味道清单 |

### 九、零拷贝资源池

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **09** | [zero-copy-pool.md](references/09-zero-copy-pool.md) | 资源池拓扑（持久段+瞬时段）+ JS 侧 `TextEncoder.encodeInto` 零拷贝注入 + Wasm 侧边界截击解析 + 帧生命周期同步 + 硬约束 [F-05]~[F-07] |

### 十、零拷贝指令总线 V3.1

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **10** | [command-bus-v3.md](references/10-command-bus-v3.md) | 双缓冲拓扑（`DoubleBuffer` 头 16 字节）+ 原子同步协议（`AcqRel` 内存序）+ JS Facade 写入循环（`DataView` 标量注入）+ Wasm 安全消费循环（单一切片派发）+ 生命周期安全契约 + 硬约束 [F-08]~[F-11] |

### 十一、工具链与全生命周期自动化 V3.2

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **11** | [toolchain-v3.md](references/11-toolchain-v3.md) | 编译期布局断言（`size_of` + `offset_of`）+ 二进制体积预算（`.wasm-size-budget.json`）+ `twiggy` 诊断 + `performance.now()` 遥测 + 生命周期检查点 + 硬约束 [F-12]~[F-15] |

### 十二、领域特定引擎 V4.1

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **12** | [domain-engines-v4.md](references/12-domain-engines-v4.md) | 三大领域引擎：像素渲染（wgpu/vello/tiny-skia 分层选型）+ 向量检索（SIMD128 + macerator）+ 高密度状态（CRDT/yrs 强制复用 + 依赖注入）+ 协作式时间分片（8ms 阈值）+ 环境纯洁原则 + 硬约束 [F-16]~[F-20] |

---

## 关系

```
rust-architecture-guide (通用宪法)
          │
          └──► rust-wasm-frontend-infra-guide (垂直深化)
                      │
                      ├── IRON-01 体积即王道 → 编译与产物控制
                      ├── IRON-02 边界零拷贝 → FFI 与跨语言边界
                      ├── IRON-03 内存分治   → 内存与生命周期
                      ├── IRON-04 跨源隔离   → 并发与事件驱动
                      │
                      ├── 通用代码 Wasm 适配
                      └── 禁止清单 + 合规自检
```

- 本指南依赖于 `rust-architecture-guide` 的 P0-P3 优先级框架和执行模式
- 本指南在 P0 安全之上为 `wasm32-unknown-unknown` 场景增加编译与边界层红线
- **两者互补使用**：通用宪法是基础，本指南是 Wasm 垂直深化附加条款

---

## 文件结构

```
rust-wasm-frontend-infra-guide/
├── SKILL.md                          # Skill 入口（Agent 指令）
├── README.md                         # 文档索引（英文）
├── README.zh-CN.md                   # 文档索引（中文）
└── references/                        # 12 份参考文档
    ├── 01-iron-rules.md              # 核心哲学与铁律
    ├── 02-build-control.md           # 编译与产物控制
    ├── 03-ffi-boundary.md            # FFI 与跨语言边界
    ├── 04-memory-lifecycle.md        # 内存与生命周期
    ├── 05-concurrency-events.md      # 并发与事件驱动
    ├── 06-wasm-adaptation.md         # 通用代码 Wasm 适配
    ├── 07-prohibitions-checklist.md  # 禁止清单与合规自检
    ├── 08-philosophy-v2.md           # 架构哲学与决策元规范
    ├── 09-zero-copy-pool.md          # 零拷贝资源池
    ├── 10-command-bus-v3.md          # 零拷贝指令总线 V3.1
    ├── 11-toolchain-v3.md            # 工具链与全生命周期自动化 V3.2
    └── 12-domain-engines-v4.md       # 领域特定引擎 V4.1
```

---

## 许可证

[MIT](../LICENSE)
