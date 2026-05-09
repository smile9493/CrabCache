# Rust 系统与云基础设施指南

[![Version](https://img.shields.io/badge/Version-v6.0.0-orange.svg)]()
[![Reference Docs](https://img.shields.io/badge/Reference-13%20Docs-blue.svg)]()
[![CI Lints](https://img.shields.io/badge/CI%20Lints-11%20Strict-red.svg)]()

**云原生基础设施 Rust 指南** — 面向数据库内核、分布式存储、高性能网关、容器运行时、eBPF 控制平面、OS 组件等长时间运行系统的垂直深化指南。

[English](README.md) | 简体中文

---

## 概述

本指南是 [`rust-architecture-guide`](../rust-architecture-guide/) 的**垂直深化补充**，针对：

- 数据库内核 & 存储引擎
- 分布式消息队列
- 高性能网关 / 代理
- 容器运行时、eBPF 控制平面
- OS 组件

**环境假设**：长时间运行节点（uptime > 1 年）、10GbE+ 网络、多 NUMA 架构。

## 核心哲学

| 原则 | 描述 |
|------|------|
| **Mechanical Sympathy** | 软件设计对齐硬件物理特性（CPU 缓存、NUMA、PMEM、内核 I/O 栈） |
| **Determinism** | 消除非确定性（时间、随机、HashMap 顺序），确保可复现状态机 |
| **Resilience** | 优雅降级 > 崩溃，背压 > OOM，结构化并发 > 泄漏 |
| **Jeet Kune Do** | 一击内存生命周期（Arena），像水流一样适配硬件通道（Allocator API） |

### Mechanical Sympathy — 顺应物理

极致的性能不是来自"奇技淫巧"，而是来自代码逻辑与底层物理硬件的深度共鸣。软件不是运行在抽象机上，而是运行在：

```
L1 Cache (32KB, 4 cycles) → L2 (256KB, 12 cycles) → L3 (共享, 40 cycles) → DRAM (200+ cycles)
NUMA Node 0 ← QPI/UPI → NUMA Node 1
NIC Ring Buffer → Kernel TCP Stack → User Space
```

性能不是优化出来的，而是**对齐**出来的。当你理解了 CPU cache line 是 64 字节、false sharing 会摧毁并发、mmap 的 page fault 代价是微秒级，你就不会再"优化"——你会**设计**出与硬件物理共振的结构。

### Determinism — 消除非确定性

分布式共识要求比特级可复现性。如果两个节点收到相同输入，输出必须比特级相同。任何非确定性都是共识的毒药——它会在 Raft log 中制造分叉，而分叉是分布式系统最深的恐惧。

**共识逻辑中禁止**：`Instant::now()`、`rand::random()`、`HashMap` 迭代顺序。

### Resilience — 吸收而非抵抗

系统必然走向熵增（故障、OOM、网络分区）。韧性的回应：
- **不抵抗**——吸收（背压优于 OOM）
- **不崩溃**——降级（优雅降级优于崩溃）
- **不死亡**——重生（K8s 重启 + 持久化状态）

### Jeet Kune Do — 一击必杀，如水之形

- **一击**：Arena 分配——一次分配，批量回收，O(1) 生命周期管理
- **如水**：Allocator API——数据流向 NUMA 本地节点，流向 PMEM 持久层，顺硬件之势而为

## 红线（绝对禁止）

| 类别 | 禁止 | 强制 |
|------|------|------|
| **通道** | `std::sync::mpsc::channel()` 无界 | `tokio::sync::mpsc::channel(LIMIT)` |
| **取消** | `select!` 中直接非幂等写 | `spawn` + `oneshot` |
| **时间** | 状态机 Apply 中使用 `Instant::now()` | Leader 提议的时间戳 |
| **关闭** | `SIGTERM` 时立即退出 | 优雅关闭（cancel token → wait inflight → fsync → exit） |
| **FFI** | `extern` 函数不加 `catch_unwind` | 捕获 panic + 返回错误码 |
| **单次请求分配** | AST 节点用全局堆 | Arena（`bumpalo`） |
| **Arena 逃逸** | 保存 Arena 指针到外部静态变量 | 需要时 Clone 到全局堆 |
| **夜间分配器** | 生产环境用 `std::alloc::Allocator` | `allocator_api2`（稳定） |
| **碎片分配** | FFI 边界直接 `alloc::alloc` | 预分配 Slab（`mmap` + `mlock`） |
| **分配 Panic** | 分配失败直接 `panic!` | `Result<T, AllocError>` + 背压 |
| **读重路径锁** | 100+ 核读路径用 `RwLock`/`Mutex` | `arc-swap`（RCU 零阻塞读） |
| **无锁回收** | 无保护直接 `drop` 共享指针 | `crossbeam-epoch` Guard |
| **内存序** | 盲目用 `Ordering::SeqCst` | `Release`+`Acquire` 对；计数器用 `Relaxed` |
| **逐字节循环** | 核心解析循环逐字节 `if byte == b'\n'` | SIMD bitmask 批量比较 |
| **AoS 聚合** | 大批量聚合用 Array of Structs | SoA（Struct of Arrays）列式布局 |

---

## 文档索引

`references/` 目录包含 **13 份深度参考文档**，覆盖云基础设施核心领域：

### 一、I/O 与零拷贝

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **01** | [io-model.md](references/01-io-model.md) | I/O 模型选型：Tokio epoll vs io_uring vs monoio 决策树 + 零拷贝管道（`splice`/`sendfile`/`copy_file_range` via `rustix`）+ `bytes::Bytes` O(1) clone + Direct I/O（`O_DIRECT` + 对齐）+ 混合运行时红线 |

### 二、背压与取消安全

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **02** | [backpressure.md](references/02-backpressure.md) | 有界通道 + `Semaphore` 全局并发限制 + HTTP 503 `Retry-After` 传播 + 取消安全（非幂等写 `spawn` + `oneshot`）+ 熔断器 |

### 三、系统调用与 eBPF

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **03** | [syscall.md](references/03-syscall.md) | 封装优先级（`rustix` → `nix` → `libc`）+ eBPF 集成（`aya`/`libbpf-rs`）+ 错误码映射 |

### 四、共识与确定性

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **04** | [consensus.md](references/04-consensus.md) | Raft/Paxos Apply 绝对确定性 + `BTreeMap`/`IndexMap` 替代 + 状态指纹验证 + 零拷贝序列化 |

### 五、韧性设计

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **05** | [resilience.md](references/05-resilience.md) | 优雅关闭流程 + Lock Poisoning 处理 + K8s 健康检查 + 失败降级矩阵 |

### 六、可观测性

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **06** | [observability.md](references/06-observability.md) | 结构化日志 + 热路径静默 + `loom` 确定性并发测试 + `turmoil` 网络故障模拟 + I/O 错误注入 |

### 七、无锁并发

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **07** | [lock-free.md](references/07-lock-free.md) | RCU 模式（`arc-swap`）+ Epoch 回收（`crossbeam-epoch`）+ 内存序精确控制 |

### 八、向量化执行

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **08** | [vectorized.md](references/08-vectorized.md) | SIMD 指令 + Bitmask 消除分支 + SoA 列式布局 + LLVM 自动向量化 |

### 九、代码规范

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **09** | [code-style.md](references/09-code-style.md) | 容量感知 + 热路径零分配 + SAFETY 注释强制 + FFI `catch_unwind` + 禁止 Mutex 跨 `.await` + 穷举模式匹配 |

### 十、CI 检查

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **10** | [ci-lints.md](references/10-ci-lints.md) | 11 项 deny-level lints + test 环境 `cfg_attr` 放宽 + Cargo.toml `[lints]` 配置 |

### 十一、高级内存

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **11** | [memory-advanced.md](references/11-memory-advanced.md) | Arena + Slab 预分配 + Allocator API + 内存耗尽背压 |

### 十二、防波堤架构

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **12** | [breakwater-pattern.md](references/12-breakwater-pattern.md) | Facade/Core 分层架构 + Facade 职责（限流、熔断、输入验证、背压、优雅降级）+ Core 不变量（无网络 I/O、无锁、纯逻辑、零拷贝）+ 边界截击协议 |

### 十三、物理可行性审计

| 编号 | 文档 | 覆盖范围 |
|------|------|---------|
| **13** | [physical-audit.md](references/13-physical-audit.md) | 设计前强制审计 + 容器内存限制审计（70% 阈值→熔断）+ 网络延迟预算（p50/p99 分布）+ NUMA 拓扑审计 + 云审计报告模板 |

---

## 关系

```
rust-architecture-guide (通用宪法)
          │
          └──► rust-systems-cloud-infra-guide (垂直深化)
                      │
                      ├── I/O 模型：Tokio vs io_uring
                      ├── 背压：有界通道 + Semaphore
                      ├── 系统调用：rustix 封装
                      ├── 共识：确定性状态机
                      ├── 韧性：优雅关闭 + 熔断
                      ├── 可观测性：tracing + metrics
                      ├── 无锁：RCU + Epoch + 内存序
                      ├── 向量化：SIMD + SoA
                      ├── 内存：Arena + Slab + Allocator API
                      ├── 防波堤：Facade/Core 分层架构
                      └── 物理审计：容器内存限制 + 网络延迟预算 + NUMA 拓扑
```

- 本指南依赖于 `rust-architecture-guide` 的 P0-P3 优先级框架和执行模式
- 本指南在 P0 安全之上为云原生场景增加系统级红线和硬件对齐约束
- **两者互补使用**：通用宪法是基础，本指南是云基础设施专用附加条款

---

## 文件结构

```
rust-systems-cloud-infra-guide/
├── SKILL.md                          # Skill 入口（Agent 指令）
├── README.md                         # 文档索引（英文）
├── README.zh-CN.md                   # 文档索引（中文）
└── references/                        # 13 份深度参考文档
    ├── 01-io-model.md               # I/O 模型与零拷贝
    ├── 02-backpressure.md           # 背压与取消安全
    ├── 03-syscall.md                # 系统调用封装
    ├── 04-consensus.md              # 共识与确定性
    ├── 05-resilience.md             # 韧性设计
    ├── 06-observability.md          # 可观测性
    ├── 07-lock-free.md              # 无锁并发
    ├── 08-vectorized.md             # 向量化执行
    ├── 09-code-style.md             # 代码规范
    ├── 10-ci-lints.md               # CI 检查
    ├── 11-memory-advanced.md        # 高级内存
    ├── 12-breakwater-pattern.md     # 防波堤架构
    └── 13-physical-audit.md         # 物理可行性审计
```

---

## 许可证

[MIT](../LICENSE)
