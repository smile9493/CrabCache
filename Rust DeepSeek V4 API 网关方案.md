# **面向 DeepSeek V4 的高性能 Rust API 网关全链路生产级方案设计与实现**

## **现代大语言模型代理基础设施的架构演进与挑战**

在生成式人工智能的生产级落地进程中，大语言模型（LLM）的推理成本与延迟始终是制约规模化应用的核心瓶颈。随着 DeepSeek V4 等新一代模型架构的发布，底层基础设施正在经历深刻的范式转移。DeepSeek V4 内置了极具创新性的硬盘级上下文缓存（Context Caching on Disk）与高度优化的注意力机制，这为 API 网关的设计提出了全新的技术要求。传统的 API 网关（如基于 NGINX、Kong 或是基于 Node.js 的轻量级方案）在处理现代 LLM 流量时，逐渐暴露出底层架构的局限性。

传统代理架构在设计之初主要针对短连接的无状态 HTTP 请求，其多进程隔离模型导致连接池复用率低下。当面对 AI 场景中高频且长时间保持的 Server-Sent Events (SSE) 流式响应时，传统网关不仅在内存安全性上存在隐患，更缺乏对大模型上下文前缀亲和性的深度感知与优化能力 1。本研究方案系统性地摒弃了传统的反向代理架构，提出采用纯 Rust 技术栈构建专用的高性能 AI API 网关。该架构的核心基座基于 Cloudflare 开源的 Pingora 网络框架，并深度整合了 Oxcache 多级精确缓存机制与基于 Qdrant 的原生语义缓存系统。整个系统的首要工程目标是通过极致的会话亲和性路由与双轨缓存防御体系，将 DeepSeek V4 的缓存命中率推至 98% 以上，在实现数量级成本削减的同时，确保网关层引入的延迟保持在亚毫秒级别。

## **DeepSeek V4 缓存机制的物理实现与经济学模型**

DeepSeek V4 架构能够以极低的资源消耗支持高达一百万（1M） Token 的上下文窗口，其核心在于对键值（KV）缓存机制的颠覆性重构。理解这一底层物理机制，是设计高效网关路由策略的先决条件。

### **混合注意力机制与存储压缩**

DeepSeek V4 引入了混合注意力机制，交替使用压缩稀疏注意力（Compressed Sparse Attention, CSA）与重度压缩注意力（Heavily Compressed Attention, HCA），并结合滑动窗口注意力（Sliding Window Attention, SWA）来处理局部上下文 3。在这种架构下，模型沿 Token 轴对 KV 缓存进行了大幅压缩。例如，CSA 会将每 4 个 Token 压缩为一个条目，而 HCA 更是采用了高达 128 倍的压缩率 3。通过这种极端的存储优化，V4-Pro 在 1M 上下文下的 KV 缓存占用仅为 1.20 GiB，远低于其前代 V3.2 架构的 10.48 GiB 4。

在 API 网关层面，这种存储结构的改变直接体现在缓存匹配规则上。DeepSeek V4 默认开启的硬盘上下文缓存要求后续请求必须与已持久化的缓存前缀单元达成“完整单元匹配”（Full Unit Matching）。由于滑动窗口机制的存在，缓存前缀不再是连续的任意切片，而是被存储为独立且完整的单元块 5。这意味着，如果后续请求只包含了前一次请求的部分内容，但未覆盖一个完整的持久化单元，则无法触发缓存命中。

### **前缀对齐与持久化边界**

系统在处理请求时，会在特定节点进行缓存单元的持久化操作。其持久化时机严格遵循以下规则：请求边界持久化（即在用户输入结束与模型输出结束的位置生成缓存前缀单元）、公共前缀检测持久化（当系统识别到多个不同请求共享同一前缀时，将该公共前缀独立持久化）、以及固定 Token 间隔持久化（针对超长输入，在特定间隔主动切分缓存单元以防无法命中）5。此外，系统采用 64 个 Token 作为一个基础存储单元，任何不足 64 Token 的独立内容均不会进入缓存生命周期 6。

为了最大化触发这种基于硬盘的上下文缓存，网关层必须承担起“流量整形”的职责。传统的随机轮询（Round-Robin）负载均衡策略会将同一用户的多轮对话或同类数据分析任务均匀打散到后端的不同模型节点上，这导致每个节点的 KV 缓存池中都只有碎片化的前缀，无法形成高度集中的公共前缀池。因此，网关必须利用一致性哈希等手段，主动为人为创造长效的缓存亲和性。

### **缓存经济学与期望成本建模**

大模型 API 的定价策略使得缓存命中率的微小提升都能转化为巨大的经济效益。根据 DeepSeek 的公开定价模型，V4-Pro 模型的缓存未命中输入价格为每百万 Token $1.74（或折扣价 $0.145），而缓存命中时的价格仅为其十分之一 7。

为了量化网关架构带来的收益，我们可以建立一个综合成本期望模型。在缺乏优化的传统网关下，单次请求的期望成本为完全的未命中计算：

![][image1]  
引入本方案的立体缓存架构（L0 本地精确缓存、L1 分布式精确缓存、以及 L2 DeepSeek 内置缓存）后，请求流量将被截留和分化。设 ![][image2] 为网关层（L0/L1）直接拦截并返回缓存的概率，此时不再消耗后端 API 费用；设 ![][image3] 为请求穿透网关但在 DeepSeek 端命中前缀缓存的概率。优化后的单次请求期望成本则大幅降低为：

![][image4]  
在实际企业级应用中，例如针对长篇法律文档的批量问答分析，系统预设的上下文模板极为庞大。通过网关预热机制（主动发起一次请求以构建 DeepSeek 端的公共前缀单元），后续的并发请求不仅能享受 ![][image2] 带来的零推理成本，穿透的流量也能以近乎 100% 的概率落入 ![][image3] 象限，整体 API 支出可轻易缩减 90% 以上。

## **Pingora 网络框架的底层并发模型与代理重构**

为了在提供复杂路由与多重缓存解析的同时保持亚毫秒级的代理延迟，网关的核心引擎选用 Cloudflare 开源的 Rust 网络框架 Pingora。Pingora 的设计初衷正是为了克服 NGINX 在超大规模并发下的架构局限性。

### **线程模型与无锁两级连接池**

NGINX 采用了经典的多进程（Worker Process）架构。在代理高频的 HTTP 流量时，这种进程隔离机制导致了一个严重的资源瓶颈：每个进程只能维护其专属的后端连接池 9。当某个进程负载骤增时，它无法“借用”相邻空闲进程已经建立好的 TLS 长连接，必须强制发起新的 TCP 和 TLS 握手，这显著增加了首字节时间（TTFB, Time To First Byte）。

Pingora 从根本上改变了这一现状。它采用了基于 Tokio 异步运行时的多线程共享内存模型，所有工作线程存在于同一个内存空间中 9。在连接池的设计上，Pingora 实现了一种精妙的“两级连接池（Two-Stage Connection Pool）”架构，以消除高并发下的锁竞争：

1. **无锁热池（Lock-Free Hot Pool）**：每个线程维护一个本地连接池。Pingora 使用底层的 AtomicPtr 类型，在 CPU 指令级别实现安全的指针交换，允许线程在没有任何互斥锁（Mutex）开销的情况下存取最常用的连接 9。这使得 90% 以上的流量可以在本地极速流转。  
2. **全局共享池（Global Shared Mutex Pool）**：仅当线程的本地无锁池耗尽，或者需要跨线程调配连接时，才会回退到使用互斥锁保护的全局共享池进行连接窃取与均衡 9。

这种架构在应对大模型 API 典型的“高并发、长连接”工作负载时，展现出了极强的统治力。在相同的流量压力下，Pingora 的 CPU 和内存资源消耗仅为传统代理的三分之一 1。

### **ProxyHttp Trait 与全生命周期拦截**

Pingora 提供了高度可编程的接口，其中最核心的便是 ProxyHttp Trait。该接口允许开发者以面向对象的方式，将自定义的安全、路由和缓存逻辑无缝注入到 HTTP 请求处理的各个阶段 11。

在构建针对 DeepSeek 的专网代理时，需要实现几个关键的回调函数。在请求接入阶段，new\_ctx 函数负责初始化一个基于请求的上下文结构体，用于在后续的各个生命周期阶段之间传递状态信息。随后，upstream\_peer 阶段接管了寻址职责，这是实现模型节点智能路由的核心。网关在这里解析客户端的认证令牌，匹配后端可用的模型资源池，并构造 HttpPeer 对象。相比于单纯地解析域名，此阶段支持动态插入 SNI（Server Name Indication）信息和配置 TLS 参数，从而确保与商业模型 API 的安全握手 13。

### **突破流式响应（SSE）的底层 Flush 缺陷**

人工智能应用普遍采用 Server-Sent Events (SSE) 协议，通过 Transfer-Encoding: chunked 来实现模型生成 Token 的逐字推送。为了在网关层实现缓存机制，同时不阻塞流式响应的实时下发，网关必须具备旁路拦截能力。

在 ProxyHttp 的 response\_body\_filter 阶段，Pingora 会暴露出从上游传回的离散数据块。网关的策略是克隆这些数据块并追加到上下文（Context）中的自建内存缓冲区，而绝不调用清空或丢弃原缓冲区的操作，从而确保 Pingora 的底层引擎能够持续将数据刷入 TCP 栈向客户端吐出 14。

然而，在底层网络栈的具体实现上存在显著的平台差异。在 macOS（Darwin 系统，Apple Silicon 芯片）环境下，Pingora 0.8.0 及其底层框架暴露出了一个严重的流处理 Bug（Issue \#841）14。在代理 SSE 响应时，尽管应用层逻辑在每次写入数据块后都显式调用了 stream.flush()，但 macOS 操作系统的 TCP 协议栈和 BufWriter 的交互存在异常。数据块会被强行保留在下游套接字的缓冲区内（例如堆积至 1460 字节的 MSS 边界），而不会即时推送给客户端 14。这导致客户端（如 curl）完全接收不到任何流式 Token，直到连接最终因超时而被强制重置，并抛出 Prematurely before response body is complete 错误 14。

此问题暴露出内核级网络编程的复杂性。由于该缺陷在 Linux 系统（如 Ubuntu 22.04）上完全不存在，流式数据可以被完美且即时地下发，因此本架构的部署规约明确规定：在生产环境与高保真预发布环境中，网关进程必须直接运行于经过调优的 Linux 内核之上；任何基于 macOS 的开发调试都需依托网络协议栈被虚拟化隔离的 Docker 容器进行，以彻底规避内核 Flush 不一致导致的流式响应挂起问题。

## **基于 Ketama 算法的极致会话亲和性路由**

在大模型的上下文缓存架构中，网关的路由决策直接决定了底层计算资源的经济效益。为了确保 DeepSeek V4 硬盘级前缀缓存被最大化触发，必须废除传统的轮询（Round-Robin）或最少连接（Least-Connections）负载均衡策略，转而采用强会话亲和性（Session Affinity）路由。

### **Ketama 一致性哈希算法的集成与原理**

本网关采用了基于 pingora-ketama 模块的一致性哈希算法。Ketama 算法最初是为 Memcached 分布式缓存设计的，其核心思想是将所有上游服务器节点映射到一个具有 ![][image5] 个离散点的环形地址空间（Continuum Hash Ring）上 15。

当一个新的 API 请求到达时，网关提取请求头中的 x-conversation-id（若无则依次降级使用 user\_id 或客户端源 IP）作为哈希键。该键同样被映射到这个环形空间中的一个确定点。算法随后沿顺时针方向在环上查找，遇到的第一个服务器节点即为该请求的路由目标 16。

### **虚拟节点与抗雪崩能力**

如果在环上为每个物理服务器仅分配一个哈希点，由于哈希函数分布的随机性，极易导致负载严重倾斜。为了解决这一问题并应对集群规模的动态变化，Ketama 算法引入了虚拟节点（Virtual Nodes）机制。默认情况下，pingora-ketama 会为服务器的每个权重单位（Weight Unit）生成 160 个虚拟节点（DEFAULT\_POINT\_MULTIPLE 常量控制）15。例如，权重为 2 的节点将在环上占据 320 个离散点。

这种高密度的虚拟映射带来了卓越的抗雪崩能力。在业务高峰期，当运维团队对代理后端进行横向扩容（如新增一组网关通道）时，在传统的哈希求余算法（![][image6]）下，节点数 ![][image7] 的改变会导致几乎所有的请求被重新路由，这对于 DeepSeek 的 L2 前缀缓存而言是毁灭性的击穿。而在 Ketama 环形结构中，新加入的服务器只会接管环上与其相邻的一小段弧形区间的请求，绝大部分哈希键与服务器的映射关系保持不变 17。这种“最小重分配”特性完美契合了模型端状态的高昂构建成本，保护了绝大多数历史上下文缓存免遭清空。

| 路由算法 | 上下文缓存连贯性 | 扩缩容数据漂移率 | 适用场景 |
| :---- | :---- | :---- | :---- |
| **Round-Robin** | 极低，请求被随机打散 | 0% (无状态影响) | 无状态计算，轻量 HTTP API |
| **哈希求余 (Modulo)** | 较高，固定节点 | 近乎 100% (全量漂移) | 节点极其稳定、不允许扩容的单体架构 |
| **Ketama 一致性哈希** | 极高，精准命中特定节点 | ![][image8] (平滑过渡) | **大语言模型推理网关**，分布式高可用缓存 |

## **Oxcache 与 Redis 构筑的异构精确缓存防御层**

为了应对局域网内部的高频确定性查询（如系统级的固定 Prompt 解析、自动摘要等），网关在 L0（进程内存）与 L1（分布式集群）层面构筑了基于 oxcache 的精确匹配防御层。其核心目的是在字符哈希完全匹配的情况下，以亚毫秒级的延迟直接拦截请求，从而彻底消除后端 API 调用的所有成本。

### **Oxcache 的生产级架构与 Moka L0 引擎**

oxcache 是 Rust 生态中专注于多级缓存协调的生产级库，提供从内存到网络的多层回退与自动晋升（Auto-Promotion）机制 19。在其 0.2.0 及更高版本中，全面支持了基于 Pub/Sub 的多实例缓存失效同步机制与预写日志（WAL）故障恢复功能 20。

在 L0 本地内存层，方案选用了 moka 引擎。Moka 采用了高并发的分片锁机制与 TinyLFU（Least Frequently Used）准入策略 21。TinyLFU 能够高效过滤掉罕见访问的长尾流量，确保有限的内存资源只被最频繁请求的热点数据（Hot Keys）所占据。由于 Moka 运行在 Pingora 网关的同进程地址空间内，缓存命中时省去了所有序列化与网络 I/O 成本，其响应时间达到了惊人的 100 纳秒以内（P99 \< 100ns）19。对于容量管理，可为 Moka 设置严格的条目上限与内存水位阈值，防止高频变动导致代理进程 Out-Of-Memory (OOM)。

### **Redis L1 的网络拓扑与多路复用连接池评估**

当 Moka 层未命中时，请求下沉至 L1 分布式 Redis 集群。这一层的引入至关重要，它使得部署在多个物理机上的 Pingora 网关实例能够共享庞大的缓存视图，极大地提升了系统的全局缓存命中率。

在 Rust 与 Redis 通信的底层实现上，工程团队面临着网络连接管理的抉择：是使用支持纯粹异步流式操作的 MultiplexedConnection，还是采用传统的 bb8 异步连接池 22。

* **MultiplexedConnection**：该模式下，应用程序与 Redis 之间仅维持一条物理 TCP 连接。底层的异步引擎通过多路复用机制，在单一通道上交织发送并发命令。这种方案的优势在于极低的内存开销与无需复杂的池化管理，连接克隆代价几乎为零 22。然而，在大模型网关场景下，由于缓存的载荷往往是包含成百上千 Token 及其元数据的巨大 JSON 结构，单一连接极易遭遇队头阻塞（Head-of-Line Blocking）。巨大的载荷会占用宝贵的信道传输窗口，导致其他轻量级查询排队等待。  
* **bb8 异步连接池**：该方案维护了多个相互独立的物理连接，支持通过配置参数控制核心池大小与空闲回收策略 24。在面对体积庞大且并发密集的 LLM 缓存数据时，独立连接可以利用现代网卡的多队列特性并行传输，避免了串行阻塞，极大提升了 L1 层的吞吐量与稳定性 25。

结合 oxcache 的智能批量写入（Batch Writes Optimization）特性 19，本方案最终确立了基于 bb8 构建 Redis 集群连接池的架构。这确保了跨实例共享的高效与低延迟，使 L1 层响应被牢牢控制在数毫秒内（P99 \< 5ms）19。

### **请求合并（Request Coalescing）与防击穿设计**

在实际生产中，当某一条极其热门的通用 Prompt（如每天早晨大批员工同时拉取的“今日日报生成”模板）缓存恰好过期失效时，瞬时并发会引发“缓存击穿”（Cache Stampede）。大量网关线程在 L0 和 L1 层同时遭遇 Miss，进而争相向后端的 DeepSeek 发起相同的高成本推理请求，导致模型队列爆满并严重超支。

oxcache 在其底层架构中通过结合 DashMap 与 Mutex 实现了“请求合并（Request Coalescing）”机制 26。当网关检测到缓存未命中时，会首先在一个全局的并发哈希表（DashMap）中注册该 CacheKey 的挂起状态（In-flight）。随后只有抢到互斥锁的第一个“先锋”请求才会被允许穿透到 DeepSeek 发起真实的网络调用。与此同时，其余数百个并发请求将进入基于事件通知的异步挂起（Await）状态。一旦先锋请求获得响应并成功回填（Backfill）到 Redis 与 Moka 缓存中，所有被挂起的请求将瞬间被唤醒，并从本地共享结构中直接提取已有的响应结果。这一机制如同防洪大坝，彻底终结了由高并发同源查询引发的模型层崩溃与财务灾难。

## **基于原生计算架构的降维打击：Qdrant 语义缓存层**

精确匹配缓存虽然高效，但只能覆盖输入字符 100% 一致的请求。在自然语言交互中，用户表达同一意图的句式千变万化（例如“解释这段代码”、“这段代码是什么意思”）。为了捕获这些广阔的“长尾”查询，网关架构在二期规划中引入了原生的语义缓存（Semantic Caching）层，以计算换取成本。

### **语义缓存的运作流转**

语义缓存放弃了基于哈希的键值查找，转而依赖向量相似度。其工作流程为：当精确缓存（L0/L1）未命中时，网关将用户原始请求文本送入轻量级嵌入模型（Embedding Model）中生成一个稠密向量（Dense Vector）。随后，该向量被用作查询参数发送至向量数据库，寻找具有相近含义的缓存条目 27。如果数据库返回的最大余弦相似度（Cosine Similarity）超过了系统设定的安全阈值（例如 0.95），则判定意图一致，直接返回数据库载荷（Payload）中绑定的 LLM 生成结果 28。此过程跳过了昂贵的生成式推理步骤，将长文本的输出开销降为零。

### **Rust 原生推理引擎的极限对决：ort 与 candle**

为了贯彻网关的高性能与内存安全设计理念，我们拒绝引入任何 Python 进程（如 FastAPI）来处理模型推理，而是坚持在 Rust 进程内嵌入计算引擎。当前生态中，用于运行 sentence-transformers 等小型嵌入模型（如 all-MiniLM-L6-v2）的主流 Rust 框架存在显著的性能分化。

* **Candle 框架**：由 Hugging Face 主导开发的纯 Rust 深度学习框架。虽然其完全摒弃了对外部 C++ 动态链接库的依赖，具备极高的部署便携性 29，但在实际的 CPU 推理基准测试中表现挣扎。由于其底层张量算子（Kernels）和计算图融合（Graph Fusion）优化尚未成熟，在处理 all-MiniLM-L6-v2（批量大小 4）时，Candle 的单次推理平均耗时高达约 122.30 毫秒，且抖动极大（标准差 43.38 毫秒）31。  
* **ONNX Runtime (ort)**：ort 是微软 ONNX Runtime 的安全 Rust 绑定实现 30。通过加载经过深度图优化（如 GELU 激活近似、Transformer 特定节点融合——优化级别 O3）的 .onnx 静态图模型，ort 能够压榨底层 CPU 的 SIMD/AVX 指令集性能 32。同等测试条件下，其推理耗时被稳稳压制在 14.34 毫秒左右 31。

| 推理引擎 | 底层依赖 | CPU 推理延迟 (Batch=4, all-MiniLM) | 优化策略集成 | 适用场景评价 |
| :---- | :---- | :---- | :---- | :---- |
| **Candle** | 纯 Rust原生 | ![][image9] 毫秒 31 | 基础，缺少深度算子融合 | 适合实验性项目、强隔离无 C++ 环境依赖 |
| **ONNX Runtime (ort)** | C++ 动态链接库 | ![][image10] 毫秒 31 | O3 级别深度计算图融合 | **生产级 API 网关推荐**，高吞吐低延迟标准 |

在网关架构中，主线程阻塞是性能杀手。基于上述基准测试结果，本方案决定采用 ort 引擎挂载 all-MiniLM-L6-v2 模型。为了进一步压缩推理延迟，这部分计算被严格卸载（Offload）到 Tokio 运行时的专属阻塞计算线程池（spawn\_blocking）中执行，从而保证了网关异步事件循环的绝对流畅。

### **Qdrant 的毫秒级检索底座**

在生成嵌入向量后，需要一个具备纳秒至毫秒级检索响应的数据库来进行比对。架构中选用的 Qdrant 是一款完全采用 Rust 开发的向量搜索引擎，其在内存控制与查询吞吐量上表现优异 33。

Qdrant 基于优化的 HNSW（分层导航小世界）算法，并支持高效的 Payload 过滤与载荷分离存储 35。当 ort 推理完毕后，网关利用 qdrant-client 发起 gRPC 调用。由于整个数据流从 Pingora 接收请求、ort 向量化、再到 Qdrant 检索返回，全部都在高效的 Rust 内存布局下流转，序列化开销被降到最低。这一由精确缓存和语义缓存构筑的联合防线，构成了网关体系的坚固长城，使得真正抵达 DeepSeek 服务器的有效载荷被极致浓缩。

## **生产级多维可观测性：Prometheus 监控与成本控制矩阵**

在一个拦截率为目标的高性能 AI 网关中，如果无法精确衡量流量走向，所有的成本削减都只是纸上谈兵。系统必须对网关层、缓存层以及下游商业 API 的运行状态建立深度的可观测性。

### **集成 pingora-prometheus 暴露指标**

Pingora 框架提供了一个即插即用的度量导出模块 pingora-prometheus。通过实例化 prometheus\_http\_service 并将其挂载到独立的监控端口（例如 0.0.0.0:9090），可以为外部的 Prometheus 抓取（Scrape）进程提供符合标准协议的 HTTP 端点 36。在此基础上，我们需要自定义一系列专属于大模型场景的计数器（Counters）、仪表盘（Gauges）和直方图（Histograms）。

### **建立 AI 业务度量体系**

由于 DeepSeek V4 在 API 响应的 usage JSON 结构中专门新增了 prompt\_cache\_hit\_tokens（命中上下文缓存的输入 Token 数）与 prompt\_cache\_miss\_tokens（未命中的输入 Token 数）两个字段 5，网关的解析模块需要在响应回传阶段实时捕获这些信息。

结合业务需求，我们构建如下的高基数（High-Cardinality）多维监控矩阵：

1. **大语言模型流量与财务指标 (LLM Traffic & Cost Metrics)**  
   * gateway\_deepseek\_input\_tokens\_total{cache\_status="hit/miss", model="v4-pro", consumer="api\_key\_xxx"}：通过提取 DeepSeek 原始响应体计算得来。该指标能精确反映不同业务方在模型内部的缓存复用情况。将其接入 Grafana，通过简单的 PromQL 结合公开计费费率，可实时展示每秒燃烧的法币成本曲线 37。  
   * gateway\_deepseek\_output\_tokens\_total：模型生成的输出 Token 计数。  
2. **多层缓存效能评估 (Cache Efficacy Metrics)**  
   * gateway\_cache\_requests\_total{tier="L0\_moka", result="hit/miss"}：评估进程内高速缓存的命中率与穿透率。  
   * gateway\_cache\_requests\_total{tier="L1\_redis", result="hit/miss"}：观测集群分布式缓存的有效性。  
   * gateway\_semantic\_cache\_requests\_total{status="hit\_above\_threshold"}：专门监控 Qdrant 向量检索的成功拦截次数。这三个指标汇总后，除以总请求量，即可得出网关层的综合阻断率。  
3. **网关服务质量与延迟监控 (QoS & Latency Metrics)**  
   * gateway\_llm\_upstream\_latency\_seconds：记录从向 DeepSeek 发起请求到接收到响应的耗时（Histogram 格式）。这有助于识别底层网络的异常拥堵 37。  
   * gateway\_stream\_first\_token\_latency\_seconds (首字延迟 TTFT)：在流式响应场景中，从请求发出到客户端接收到第一个 chunk 的绝对耗时 37。这是评估终端用户“等待焦躁感”的最核心产品级指标。  
   * gateway\_cache\_fetch\_latency\_seconds：记录读取 Redis L1 缓存或执行 ort 模型推理的消耗时间，确保代理层引入的延迟不会反向劣化体验 37。

借助动态注入请求维度的头部信息（如 x-tenant-id），度量系统实现了租户隔离级的数据溯源。运维人员可以通过 Grafana 看板清晰掌握全局架构中缓存命中的经济效用比，精准锁定哪些业务场景因为系统提示词的变动过于频繁而导致了模型端前缀缓存失效，从而反向指导开发团队进行提示词工程的重构与静态化剥离 39。

## **部署规划与系统级参数调优**

这套面向大模型网关的复杂体系，在步入生产环境前必须进行严格的资源规划和系统级底层调优，以最大化发挥 Rust 无畏并发的性能上限。

### **网络拓扑与资源评估**

系统采用完全解耦的架构，确保网关的 I/O 阻塞、CPU 的张量计算与数据库的内存存储各司其职，互不干扰。

* **Pingora 代理层**：部署 2 到 3 个 Docker 容器化网关实例（基于最小化的 Linux 发行版，如 Debian Bookworm 或 Alpine）。每个实例分配 4 核 CPU 与 4GB 内存。由于 Pingora 本身的内存开销极小，这些资源足以支撑高频的 TLS 握手解码以及 ort 引擎在此之上的局部矩阵运算负载。  
* **缓存集群**：L1 缓存采用 Redis 单节点主从配置（1GB 内存配置，配置 LRU 策略），专门存储基于 Hash 的精确响应结构；独立部署 Qdrant 服务实例用作语义库（内存 2GB 足以支撑百万级语句的 HNSW 索引与 Payload 存储）。

### **系统内核与内存分配器极速调优**

在高并发场景下，应用层的瓶颈往往转移到系统底层的内存分配器上。由于 Pingora 大量利用多线程事件循环，以及我们在响应解析中对字节数组（Bytes）和 JSON 的频繁分配，默认的系统分配器极易产生严重的锁竞争。因此，必须在网关代码入口处注入全局的高性能内存分配器。例如使用 jemalloc 或是 mimalloc，这能极大地降低碎片率，并在高压基准测试下（如 wrk）将网关的吞吐上限抬升 10% 以上。

此外，需要对承载网关的 Linux 服务器执行深度内核参数（Sysctl）调整。鉴于我们维护了巨大的前后端连接数（客户端长连接以及 bb8 维持的后端空闲池），必须扩大临时端口范围（net.ipv4.ip\_local\_port\_range）并提升 SOMAXCONN 积压队列深度（net.core.somaxconn），防止在突发流量（Traffic Spikes）抵达时由于操作系统队列满载而发生的请求被拒现象。

## **结论**

在生成式 AI 日益普及的当下，计算成本成为了基础设施架构中最核心的考量指标。本研究方案深入剖析了 DeepSeek V4 基于硬盘的 KV 前缀缓存及其计费机制，创造性地将其物理规律反向转化为网关路由设计的依据。通过引入基于 Cloudflare Pingora 的高性能纯 Rust 代理框架，我们彻底摆脱了传统多进程网关在并发连接控制上的弊病，不仅成功修补了底层操作系统的流式 Flush 缺陷，更实现了极致低延迟的事件驱动循环。

通过采用基于 Ketama 算法的一致性哈希路由，我们将散乱的请求前缀高度聚合，完美契合了模型端的滑动窗口与持久化边界逻辑。同时，结合 oxcache 主导的 L0/L1 内存分布式异构防击穿缓存，以及基于 ONNX Runtime (ort) 和 Qdrant 构筑的降维语义识别网络，该网关架构构筑了一张密不透风的流量过滤网。搭配基于 Prometheus 的全维度 Token 成本追踪看板，整个系统真正做到了在保持用户交互流畅度的同时，将不可控的 LLM 推理开支转化为高度可预期且被极限压缩的边际成本，为企业构建低成本、高可用的 AI 应用体系树立了工程标杆。

#### **Works cited**

1. How we built Pingora, the proxy that connects Cloudflare to the Internet, accessed May 9, 2026, [https://blog.cloudflare.com/how-we-built-pingora-the-proxy-that-connects-cloudflare-to-the-internet/](https://blog.cloudflare.com/how-we-built-pingora-the-proxy-that-connects-cloudflare-to-the-internet/)  
2. How Cloudflare's Pingora Uses Rust to Replace NGINX: A Game-Changer for Web Performance \- Aarambh Dev Hub, accessed May 9, 2026, [https://aarambhdevhub.medium.com/how-cloudflares-pingora-uses-rust-to-replace-nginx-a-game-changer-for-web-performance-e5bf0b1416f2](https://aarambhdevhub.medium.com/how-cloudflares-pingora-uses-rust-to-replace-nginx-a-game-changer-for-web-performance-e5bf0b1416f2)  
3. DeepSeek-V4: a million-token context that agents can actually use \- Hugging Face, accessed May 9, 2026, [https://huggingface.co/blog/deepseekv4](https://huggingface.co/blog/deepseekv4)  
4. The exact KV cache usage of DeepSeek V4 : r/LocalLLaMA \- Reddit, accessed May 9, 2026, [https://www.reddit.com/r/LocalLLaMA/comments/1svzlog/the\_exact\_kv\_cache\_usage\_of\_deepseek\_v4/](https://www.reddit.com/r/LocalLLaMA/comments/1svzlog/the_exact_kv_cache_usage_of_deepseek_v4/)  
5. Context Caching \- DeepSeek API Docs, accessed May 9, 2026, [https://api-docs.deepseek.com/guides/kv\_cache](https://api-docs.deepseek.com/guides/kv_cache)  
6. DeepSeek API introduces Context Caching on Disk, cutting prices by an order of magnitude, accessed May 9, 2026, [https://api-docs.deepseek.com/news/news0802](https://api-docs.deepseek.com/news/news0802)  
7. DeepSeek V4 Pro Pricing Guide 2026: Pricing, Providers & Cost Comparison \- DeepInfra, accessed May 9, 2026, [https://deepinfra.com/blog/deepseek-v4-pro-pricing-guide-2026-providers-cost-analysis](https://deepinfra.com/blog/deepseek-v4-pro-pricing-guide-2026-providers-cost-analysis)  
8. Models & Pricing \- DeepSeek API Docs, accessed May 9, 2026, [https://api-docs.deepseek.com/quick\_start/pricing](https://api-docs.deepseek.com/quick_start/pricing)  
9. Pingora Deep Dive: The Rust Proxy That Retired NGINX \- DEV Community, accessed May 9, 2026, [https://dev.to/kanywst/pingora-the-rust-proxy-that-retired-nginx-2hd1](https://dev.to/kanywst/pingora-the-rust-proxy-that-retired-nginx-2hd1)  
10. Pingora async runtime and threading model \- Ricardo Pallás, accessed May 9, 2026, [https://rpallas.xyz/pingora-internals-1-async-runtime/](https://rpallas.xyz/pingora-internals-1-async-runtime/)  
11. pingora/docs/user\_guide/phase.md at main \- GitHub, accessed May 9, 2026, [https://github.com/cloudflare/pingora/blob/main/docs/user\_guide/phase.md](https://github.com/cloudflare/pingora/blob/main/docs/user_guide/phase.md)  
12. pingora/pingora-proxy/src/proxy\_trait.rs at main · cloudflare/pingora \- GitHub, accessed May 9, 2026, [https://github.com/cloudflare/pingora/blob/main/pingora-proxy/src/proxy\_trait.rs](https://github.com/cloudflare/pingora/blob/main/pingora-proxy/src/proxy_trait.rs)  
13. pingora/docs/quick\_start.md at main \- GitHub, accessed May 9, 2026, [https://github.com/cloudflare/pingora/blob/main/docs/quick\_start.md](https://github.com/cloudflare/pingora/blob/main/docs/quick_start.md)  
14. SSE (Server-Sent Events) response body not flushed to downstream on macOS · Issue \#841 · cloudflare/pingora \- GitHub, accessed May 9, 2026, [https://github.com/cloudflare/pingora/issues/841](https://github.com/cloudflare/pingora/issues/841)  
15. pingora\_ketama \- Rust \- Docs.rs, accessed May 9, 2026, [https://docs.rs/pingora-ketama](https://docs.rs/pingora-ketama)  
16. Consistent Hashing: Algorithmic Tradeoffs | by Damian Gryski | Medium, accessed May 9, 2026, [https://dgryski.medium.com/consistent-hashing-algorithmic-tradeoffs-ef6b8e2fcae8](https://dgryski.medium.com/consistent-hashing-algorithmic-tradeoffs-ef6b8e2fcae8)  
17. Pingora Guide \- How To Make A Programmable API Gateway \- DEV Community, accessed May 9, 2026, [https://dev.to/warren\_jitsing\_dd1c1d6fc6/pingora-guide-how-to-make-a-programmable-api-gateway-1oim](https://dev.to/warren_jitsing_dd1c1d6fc6/pingora-guide-how-to-make-a-programmable-api-gateway-1oim)  
18. libketama: Consistent Hashing library for memcached clients | Richard Jones, accessed May 9, 2026, [https://www.metabrew.com/article/libketama-consistent-hashing-algo-memcached-clients](https://www.metabrew.com/article/libketama-consistent-hashing-algo-memcached-clients)  
19. oxcache \- crates.io: Rust Package Registry, accessed May 9, 2026, [https://crates.io/crates/oxcache](https://crates.io/crates/oxcache)  
20. Oxcache is a high-performance, production grade Rust multi-level cache library that provides a two-layer caching architecture of L1 (Moka) and L2 (Redis). · GitHub, accessed May 9, 2026, [https://github.com/Kirky-X/oxcache](https://github.com/Kirky-X/oxcache)  
21. How to Implement Caching Strategies in Rust \- OneUptime, accessed May 9, 2026, [https://oneuptime.com/blog/post/2026-02-01-rust-caching-strategies/view](https://oneuptime.com/blog/post/2026-02-01-rust-caching-strategies/view)  
22. redis \- Rust \- Docs.rs, accessed May 9, 2026, [https://docs.rs/redis/latest/redis/](https://docs.rs/redis/latest/redis/)  
23. redis \- Rust, accessed May 9, 2026, [https://tg-rs.github.io/carapax/redis/index.html](https://tg-rs.github.io/carapax/redis/index.html)  
24. How to Build Connection Pools with bb8 and deadpool in Rust \- OneUptime, accessed May 9, 2026, [https://oneuptime.com/blog/post/2026-01-25-connection-pools-bb8-deadpool-rust/view](https://oneuptime.com/blog/post/2026-01-25-connection-pools-bb8-deadpool-rust/view)  
25. Rust Connection Pools: Keeping SQL Server Cool Under Pressure | by Luis Lema | Medium, accessed May 9, 2026, [https://medium.com/@lemalcs/rust-connection-pools-keeping-sql-server-cool-under-pressure-5f56c335e988](https://medium.com/@lemalcs/rust-connection-pools-keeping-sql-server-cool-under-pressure-5f56c335e988)  
26. multi\_tier\_cache \- Rust \- Docs.rs, accessed May 9, 2026, [https://docs.rs/multi-tier-cache](https://docs.rs/multi-tier-cache)  
27. Semantic Cache: Accelerating AI with Lightning-Fast Data Retrieval \- Qdrant, accessed May 9, 2026, [https://qdrant.tech/articles/semantic-cache-ai-data-retrieval/](https://qdrant.tech/articles/semantic-cache-ai-data-retrieval/)  
28. Top AI Gateways for Semantic Caching in 2026 \- Maxim AI, accessed May 9, 2026, [https://www.getmaxim.ai/articles/top-ai-gateways-for-semantic-caching-in-2026/](https://www.getmaxim.ai/articles/top-ai-gateways-for-semantic-caching-in-2026/)  
29. Future of \`candle-transformers\` / long-term plans · Issue \#1186 \- GitHub, accessed May 9, 2026, [https://github.com/huggingface/candle/issues/1186](https://github.com/huggingface/candle/issues/1186)  
30. Running sentence transformers model in Rust? \- Reddit, accessed May 9, 2026, [https://www.reddit.com/r/rust/comments/1hyfex8/running\_sentence\_transformers\_model\_in\_rust/](https://www.reddit.com/r/rust/comments/1hyfex8/running_sentence_transformers_model_in_rust/)  
31. Candle Inference \~8.5x Slower Than PyTorch on CPU · Issue \#2877 \- GitHub, accessed May 9, 2026, [https://github.com/huggingface/candle/issues/2877](https://github.com/huggingface/candle/issues/2877)  
32. Building Sentence Transformers in Rust: A Practical Guide with Burn, ONNX Runtime, and Candle \- DEV Community, accessed May 9, 2026, [https://dev.to/mayu2008/building-sentence-transformers-in-rust-a-practical-guide-with-burn-onnx-runtime-and-candle-281k](https://dev.to/mayu2008/building-sentence-transformers-in-rust-a-practical-guide-with-burn-onnx-runtime-and-candle-281k)  
33. Semantic Caching \- Bifrost AI Gateway, accessed May 9, 2026, [https://docs.getbifrost.ai/features/semantic-caching](https://docs.getbifrost.ai/features/semantic-caching)  
34. Semantic Search As You Type \- Qdrant, accessed May 9, 2026, [https://qdrant.tech/articles/search-as-you-type/](https://qdrant.tech/articles/search-as-you-type/)  
35. DistX: High-performance vector database in Rust \- 6x faster search than Qdrant, accessed May 9, 2026, [https://users.rust-lang.org/t/distx-high-performance-vector-database-in-rust-6x-faster-search-than-qdrant/137032](https://users.rust-lang.org/t/distx-high-performance-vector-database-in-rust-6x-faster-search-than-qdrant/137032)  
36. pingora/docs/user\_guide/prom.md at main \- GitHub, accessed May 9, 2026, [https://github.com/cloudflare/pingora/blob/main/docs/user\_guide/prom.md](https://github.com/cloudflare/pingora/blob/main/docs/user_guide/prom.md)  
37. Monitor AI LLM metrics \- AI Gateway \- Kong Docs, accessed May 9, 2026, [https://developer.konghq.com/ai-gateway/monitor-ai-llm-metrics/](https://developer.konghq.com/ai-gateway/monitor-ai-llm-metrics/)  
38. Prometheus Grafana Integration \- TrueFoundry Docs, accessed May 9, 2026, [https://www.truefoundry.com/docs/ai-gateway/prometheus-grafana-integration](https://www.truefoundry.com/docs/ai-gateway/prometheus-grafana-integration)  
39. Optimize cost and performance | Grafana Cloud documentation, accessed May 9, 2026, [https://grafana.com/docs/grafana-cloud/machine-learning/ai-observability/guides/cost-optimization/](https://grafana.com/docs/grafana-cloud/machine-learning/ai-observability/guides/cost-optimization/)

[image1]: <data:image/png;base64,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>

[image2]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEEAAAAZCAYAAABuKkPfAAAC/ElEQVR4Xu2YTahNURTHl6SIfPekiCSSgQEGrzAiDJSYKGWiiDKQUKRur4RSZKJQbyQTMylfIQYUIzGSgVJCUYqifKxfa6131t3Pe71ud3A7zq/+3bPX3ufsvc9ea+19rkhDQ0NDw6hMUM1RzS30X3FM9WcUDaqWDrWuOd/FJl1yRsy+tqyoI0z0d2lUtojVXSgr6sYMsYneLCuU62J1e8uKurFCbKLkh5JPYnWzyoq6cUX1TjVPqt1iv+qtWE6YXDWtJ4TCMxm+K5Afjqd2tYZQ+CrtoTBe9Uv1M9lqDecAVp5QyMT5YWVh75Q+1f3S2CtEKEws7LErLCvsnbJeetizRjofYKOOnPEvloidIUpIqptTmdDCC+6qHqimpzogEW9VzU827s/lWKB1qqnJzr2r/DeD99I2mCbD2wwxTmyi7AwlkSCneBmPWe7XC1WXVTvFdpDTbmcXuaHaoXqluiM2mXNiL/W5ao9Yv3BY9drb84KASRGKb7xM/+xewHhO+jW8UB1SPVF9cRuT5f5H0Uj5INZHG3wg5Z0glBueddsp1wK3n5eqQ+C4vVq1TSyZBiTbfMB6r1qUyhtULb/eqLot9sIOqq6pnnodL5rnA5PjmbyYh2IrDLwkXjrsU/WLjSugbnYqjxlWC5e/pDqR7HhNPllGB9iYaPBNqqSKO1Of8w4D/yi24rul3c3po+XXHNdjAgNizyS/5BDGSwdTuSWVdxPOV6Xyvq7A5Fh1YECx2oRFuG10zC+TY9CRJw6IDYj6CDUgdwRxcMNjeW5wxH+PinlaEN643cvcQxvAk9b4ddd4KdbBTLEtL1z1lthqMEEGSxvcEnhRhALuG2FF6EV4TBILuSBChy/XH24j1hf79SbVZ7/meUyaUIqdDO+if/q7Jx2GwliIk2Z2MwYaLs+3RnZ/dogyQ+MJI/2Bwy4S3kGbcgsH+qB/6vK3DWWezSJkj+kKZP/YFsvk0ysQRqw+Y70o5rldBfdiO3qs2iXmxr0G3sYYUU62bfwFIwmeo9ttDJQAAAAASUVORK5CYII=>

[image3]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEUAAAAZCAYAAABnweOlAAAC2klEQVR4Xu2XS6hNURjH//LIK88iIc9IBhIZIQNMxMREeUwkMlAyUF5dA8lzYIRIBgZKysBAKacYKBMTJVJXiYGkDBTy+H59a929z3fP2fde3bidc3717+77X2vvvfZa3/etdaQOHTp06PBPGWmabpoR1NYcMf2u0A3T4p7ebcZX+SREzsj9NbGhHeDDf0XT2CxvuxQbWp3J8g+/HxuMO/K2vbGh1Vkm/3DqS+SjvG1qbGh1rpnemWap2I32m97Ka8q4omt7QOo8U+9dh/pytNSvrSB1vqg+dYabfpq+l7xm0HdaNIcYpP6kaFbBOYTIIHXK5PPLiuBH6PchmkOMH6bV0awip87o4OddZ0nwI/SrRXOI0W2aGc0qmp1P8Gij5jRiozyKvpm2lHwK9UrT7JLXyB8mT9216XqReu9wE+XviT4LuK6fPot6OF3zjg2ltoYwGD6cnSeSC+749D8RtVT+ccdM2+W71icV0UTba9MB0yMVA2jko4ume/IU5pmvTKvSPcA7d5meyt8N7ITPTbtNT/rhM8718jGck7+vIfzgK+80WdtKfc4n73TSnOQTGV3pmpDMJ10+kuIMDOCB/J5m/gl5FNZUpC6p+CZdE4XXTQvkq8+9+1RENffclUdTMz/vrFfkC0ekHEz9/ooc0ldNx0t+uWjxN6cOUcMk8lG3TRP68GG+6ov0C9PjdM2Olhcr/+56mP5/KZ+wMX34TASR3G06mbxBh1RiAITqCPnKTjGdNd1SfcFlqx5b4cMp+YAzn03LTXtURBfw84MIYMJulnyiZ1SFTxR3J492ImdT+n/QIHpq8snZKh/4PNNleeq97+npKbezwucZNRX1bKHpkPwdRBerDvTbka675OmQoRYRrc38mnzhgEkhgigLg048rOVCnKFeRQ+iT9oxqRRBnsdzy+T3RB94VjxCQPSJjPL9AzrA/Q9y6gzo/NDKzJUXcHShvun/8AdpIKpvZ2hvOQAAAABJRU5ErkJggg==>

[image4]: <data:image/png;base64,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>

[image5]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABgAAAAXCAYAAAARIY8tAAABcUlEQVR4Xu2TPyjFURTHj6IUZfDyp7yBRRaUbAallMTwkMFmMtheUUqRzSSDQUoGG5n0DNLbDUaDLGJW6qmnxPfrnvuc3+WV33u/srxPffqde07d3/md370iNRJmEu7Bcdhg8r1wG2Zhp8nHohkuwzZ4D3OmdgoH4A18M/lYjMIz2AjX4Ifmh8S9sB3WwWM4obUILG7CfTgHm6Llrzo3JxewoHELPBT3heQITmlcgptemTW7YIczJkeW4ANcl58NkHn4Lq6ZCOxm0azT8A4+wm6T95yIG0vILdwIk4TdUj8CsqC5VV2PwS6NOXc25U8Mx3Qu350P6rPEE3wNcvYFnG9eY8IZs5bS9Q7MaEyGTVyWXXGbTMN6eAD7tbalNcL7UBT3byjH2qO1srBTbjAS5PvgLGwN8rF5FncaEoc/61L+OMNK4H2wx48/11+gquDxWoEdQf5afrmVlcAby3PNL7C+iDvzVeMvWmheEhpRjf/jExU+RwNMO0KtAAAAAElFTkSuQmCC>

[image6]: <data:image/png;base64,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>

[image7]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABIAAAAYCAYAAAD3Va0xAAABCklEQVR4Xu2SMWpCQRCGR7RQEGxEEQXB2iNYpsgFAuksPYKQKofwADZiYyuEVJZeQFIkVYhpU6VV///Nru4O62tFeB984Jt5zs7MW5GCm1GFncBynM6eWyaWpC5aYA+PcAsbQd4X+oX/sC/6n6t8ihY5wEeTIxs4tUFLRfQldsKu/uJ0xgds26ClCR/cbxailpXogblM5PLSq2ih53NWpAdHwXMSFuBpnqHoaG+w5mLsll3nwpN2JsZls6uZ6PVYx+k0HCvsiPil/8AB/IrTafg1/KJDWITF5nARp9JwrK4Nio7FQrxX7DoXLvPFBh38CEvRg64umtf+Cb7Dbzh2MQuXzrFKNlFwr5wACyYqq+CtpAsAAAAASUVORK5CYII=>

[image8]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACUAAAAZCAYAAAC2JufVAAABbklEQVR4Xu2VzysFURTHj1JIURKJjSIRWdgqGzt5lkjZkqWFLWv+Az8WdpaWrP0DFsrGgrKxEmVBie/XuePde3o0V80di/nUpzdz5s6bM+eee0ekoiIXXXARPsI7c600mNQCPJF/lFTGsVRJ5aNKKi9RSTXBNbgLW801S58NRMCk7m2wEZ2iS/XD+Qa3ghEhUzaQgx54IfVnMLH1YIRhX7SkvLFZ9G3e4Zg/yNELR22wCDhtjViGT/AWnoq+4ZU/oEhmbMCjA17CZ7gB28LLxTIi2kusxqr8/vBJG/iB7Uhb9DZlDr6ITtGRaGI3oivSwp77S6NHw0bnw3yYEBNj9YZFt4EDd56EeRtwsJ/2JNwq2FexdEv9P8a9OLedLD7hxZOxI7rKr0W3lQxW385SMg7hgGhVlrz4pnecHFaEn69XeObFp73j5My6X1aJ1eJC4pehtKlrh/3umP3EvhqEK98jSqBmzofgAzw38S8+AaqvSaZ7rFVZAAAAAElFTkSuQmCC>

[image9]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEEAAAAUCAYAAADStFABAAAAxElEQVR4Xu2VMQ5BQRCGR6LR0BEHEKFROIADiANotBIHcAk3IAqd0lEUr1RwBBKNRPgnO8XuFPST/0u+Yv83zc6bzIoQQgghhJC/tOARfswXXBcVJWMfRGALb7AN6/AA33CYFxkdOPBhBJY+MObwDq/wJGlKqrwgEhMfZDThGT7gCjbKz7HoS9oF+rcX8vuyIx9EYAqfkkZ+L6kRF1jLiwzdGWEXo14uRxugjdDp6MEu3Nk5JDMfGLoPNlI+nboXCCEx+QLmRR9yXGrIogAAAABJRU5ErkJggg==>

[image10]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADgAAAAUCAYAAADY6P5TAAAAxUlEQVR4XmNgGAWjYBSMglEwCkbBsAf8QLwCiP9D8S8gLkNRgQqM0QUGO5gFxA+BWAyIWYB4IRD/BWItZEVQIA7EmuiCgx2kowtAQSQQfwTiB0C8gQESu1eQFQwVYI8ugAT4gPgCEH8C4iwg5kSVHjpAnQGS90CxFMeA3yP66AKDHXgD8VcGSDKcywDx5B0gZkRWBAWgPDokCxmQw5EByHMgT4JiVRWIJYF4NpQ/5IAvugAUgPJfNwNq9QHKh6NgFIwC6gAA18Yfcvj7mwkAAAAASUVORK5CYII=>