use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    ZhCN,
    EnUS,
}

impl Locale {
    pub fn label(&self) -> &'static str {
        match self {
            Locale::ZhCN => "中文",
            Locale::EnUS => "EN",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Locale::ZhCN => Locale::EnUS,
            Locale::EnUS => Locale::ZhCN,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Translations {
    pub locale: Locale,
}

impl Translations {
    pub const fn sidebar_brand() -> &'static str { "CrabCache" }
    pub fn sidebar_subtitle(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "DeepSeek V4 网关",
            Locale::EnUS => "DeepSeek V4 Gateway",
        }
    }
    pub fn sidebar_overview(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "概览",
            Locale::EnUS => "Overview",
        }
    }
    pub fn sidebar_keys(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥与预算",
            Locale::EnUS => "Keys & Budgets",
        }
    }
    pub fn sidebar_routing(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "路由与缓存",
            Locale::EnUS => "Routing & Cache",
        }
    }
    pub fn sidebar_models(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型列表",
            Locale::EnUS => "Models",
        }
    }
    pub fn sidebar_logs(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "日志与追踪",
            Locale::EnUS => "Logs & Traces",
        }
    }
    pub fn sidebar_upstream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游配置",
            Locale::EnUS => "Upstream",
        }
    }
    pub fn sidebar_online(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "网关运行中",
            Locale::EnUS => "Gateway Online",
        }
    }

    pub fn theme(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "切换主题",
            Locale::EnUS => "Theme",
        }
    }

    pub const fn not_found_title() -> &'static str { "404" }
    pub fn not_found_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "页面未找到",
            Locale::EnUS => "Page not found",
        }
    }
    pub fn not_found_back(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "返回仪表盘",
            Locale::EnUS => "Back to Dashboard",
        }
    }

    pub fn overview_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "概览仪表盘",
            Locale::EnUS => "Overview Dashboard",
        }
    }
    pub fn overview_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "实时网关健康状态与成本效率指标",
            Locale::EnUS => "Real-time gateway health and cost efficiency metrics.",
        }
    }
    pub const fn overview_qps() -> &'static str { "QPS" }
    pub fn overview_qps_sub(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "每秒请求数",
            Locale::EnUS => "Requests per second",
        }
    }
    pub const fn overview_tps() -> &'static str { "TPS" }
    pub fn overview_tps_sub(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "每秒 Token 数",
            Locale::EnUS => "Tokens per second",
        }
    }
    pub fn overview_active_keys(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "活跃密钥",
            Locale::EnUS => "Active Keys",
        }
    }
    pub fn overview_active_keys_sub(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "使用中的 API 密钥",
            Locale::EnUS => "API keys in use",
        }
    }
    pub fn overview_uptime(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "运行时间",
            Locale::EnUS => "Uptime",
        }
    }
    pub fn overview_uptime_sub(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "自重启以来小时数",
            Locale::EnUS => "Hours since restart",
        }
    }
    pub fn overview_cache_hit_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存命中分布",
            Locale::EnUS => "Cache Hit Distribution",
        }
    }
    pub const fn overview_l0_label() -> &'static str { "L0 (Moka)" }
    pub const fn overview_l1_label() -> &'static str { "L1 (Redis)" }
    pub const fn overview_l2_label() -> &'static str { "L2 (Semantic)" }
    pub fn overview_miss_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "未命中",
            Locale::EnUS => "Miss",
        }
    }
    pub fn overview_hit_rate(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "总体命中率",
            Locale::EnUS => "Overall Hit Rate",
        }
    }
    pub fn overview_cost_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "成本节省分析",
            Locale::EnUS => "Cost Savings Analysis",
        }
    }
    pub fn overview_cost_standard(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "标准 API 费用",
            Locale::EnUS => "Standard API Cost",
        }
    }
    pub fn overview_cost_with_cache(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "使用 CrabCache",
            Locale::EnUS => "With CrabCache",
        }
    }
    pub fn overview_cost_saved(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "累计节省",
            Locale::EnUS => "Total Saved",
        }
    }
    pub fn overview_latency_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟分解 (P99)",
            Locale::EnUS => "Latency Breakdown (P99)",
        }
    }
    pub const fn overview_latency_l0() -> &'static str { "L0 Moka" }
    pub const fn overview_latency_l1() -> &'static str { "L1 Redis" }
    pub const fn overview_latency_l2() -> &'static str { "L2 Qdrant" }
    pub fn overview_latency_upstream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游服务",
            Locale::EnUS => "Upstream",
        }
    }
    pub fn overview_load_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "加载指标失败",
            Locale::EnUS => "Failed to load metrics",
        }
    }

    pub fn keys_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥与预算",
            Locale::EnUS => "Keys & Budgets",
        }
    }
    pub fn keys_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管理 API 密钥、速率限制与成本追踪",
            Locale::EnUS => "Manage API keys, rate limits, and cost tracking.",
        }
    }
    pub fn keys_new_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "+ 新建密钥",
            Locale::EnUS => "+ New Key",
        }
    }
    pub fn keys_create_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "创建 API 密钥",
            Locale::EnUS => "Create API Key",
        }
    }
    pub fn keys_name_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥名称",
            Locale::EnUS => "Key Name",
        }
    }
    pub fn keys_rpm_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "RPM 限制",
            Locale::EnUS => "RPM Limit",
        }
    }
    pub fn keys_budget_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "月度 Token 预算",
            Locale::EnUS => "Monthly Token Budget",
        }
    }
    pub fn keys_creating(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "创建中...",
            Locale::EnUS => "Creating...",
        }
    }
    pub fn keys_create_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "创建",
            Locale::EnUS => "Create",
        }
    }
    pub fn keys_cancel(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "取消",
            Locale::EnUS => "Cancel",
        }
    }
    pub fn keys_col_name(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "名称",
            Locale::EnUS => "Name",
        }
    }
    pub fn keys_col_key(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥",
            Locale::EnUS => "Key",
        }
    }
    pub const fn keys_col_rpm() -> &'static str { "RPM" }
    pub fn keys_col_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已用 Token",
            Locale::EnUS => "Tokens Used",
        }
    }
    pub fn keys_col_cost(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "费用",
            Locale::EnUS => "Cost",
        }
    }
    pub fn keys_col_status(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "状态",
            Locale::EnUS => "Status",
        }
    }
    pub fn keys_status_active(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "活跃",
            Locale::EnUS => "Active",
        }
    }
    pub fn keys_status_revoked(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已吊销",
            Locale::EnUS => "Revoked",
        }
    }
    pub fn keys_revoke_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "吊销",
            Locale::EnUS => "Revoke",
        }
    }
    pub fn keys_empty(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "尚未配置 API 密钥",
            Locale::EnUS => "No API keys configured yet.",
        }
    }
    pub fn keys_load_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "加载密钥失败",
            Locale::EnUS => "Failed to load keys",
        }
    }

    pub fn routing_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "路由与缓存策略",
            Locale::EnUS => "Routing & Cache Policy",
        }
    }
    pub fn routing_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "配置缓存 TTL、语义阈值与会话亲和性",
            Locale::EnUS => "Configure cache TTLs, semantic thresholds, and session affinity.",
        }
    }
    pub fn routing_cache_config_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存 TTL 配置",
            Locale::EnUS => "Cache TTL Configuration",
        }
    }
    pub fn routing_l0_ttl(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "L0 (Moka) TTL",
            Locale::EnUS => "L0 (Moka) TTL",
        }
    }
    pub fn routing_l1_ttl(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "L1 (Redis) TTL",
            Locale::EnUS => "L1 (Redis) TTL",
        }
    }
    pub fn routing_save(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存更改",
            Locale::EnUS => "Save Changes",
        }
    }
    pub fn routing_saved(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已保存！",
            Locale::EnUS => "Saved!",
        }
    }
    pub fn routing_semantic_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "语义缓存阈值",
            Locale::EnUS => "Semantic Cache Threshold",
        }
    }
    pub fn routing_similarity(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "相似度阈值",
            Locale::EnUS => "Similarity Threshold",
        }
    }
    pub fn routing_loose(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "0.80 (宽松)",
            Locale::EnUS => "0.80 (Loose)",
        }
    }
    pub fn routing_strict(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "0.99 (严格)",
            Locale::EnUS => "0.99 (Strict)",
        }
    }
    pub fn routing_impact_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "预估影响",
            Locale::EnUS => "Estimated Impact",
        }
    }
    pub fn routing_impact_high(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "高精度，较少语义匹配",
            Locale::EnUS => "High precision, fewer semantic matches",
        }
    }
    pub fn routing_impact_balanced(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "精度与召回平衡",
            Locale::EnUS => "Balanced precision and recall",
        }
    }
    pub fn routing_impact_loose(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "高召回，可能包含不太相关的匹配",
            Locale::EnUS => "Higher recall, may include less relevant matches",
        }
    }
    pub fn routing_affinity_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "会话亲和性 (Ketama 环)",
            Locale::EnUS => "Session Affinity (Ketama Ring)",
        }
    }
    pub fn routing_active_backends(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "活跃后端",
            Locale::EnUS => "Active Backends",
        }
    }
    pub fn routing_total_backends(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "总后端数",
            Locale::EnUS => "Total Backends",
        }
    }
    pub fn routing_distribution(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求分布",
            Locale::EnUS => "Request Distribution",
        }
    }
    pub fn routing_connection_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "连接保活配置",
            Locale::EnUS => "Connection Keepalive",
        }
    }
    pub fn routing_tcp_keepalive_idle(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "TCP Keepalive 空闲",
            Locale::EnUS => "TCP Keepalive Idle",
        }
    }
    pub fn routing_tcp_keepalive_interval(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "TCP Keepalive 间隔",
            Locale::EnUS => "TCP Keepalive Interval",
        }
    }
    pub fn routing_tcp_keepalive_count(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "TCP Keepalive 次数",
            Locale::EnUS => "TCP Keepalive Count",
        }
    }
    pub fn routing_idle_timeout(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "连接池空闲超时",
            Locale::EnUS => "Idle Timeout",
        }
    }
    pub fn routing_h2_ping_interval(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "HTTP/2 PING 间隔",
            Locale::EnUS => "HTTP/2 PING Interval",
        }
    }
    pub fn routing_connection_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "配置上游连接保活策略，维持长连接活跃",
            Locale::EnUS => "Configure upstream keepalive to maintain active connections",
        }
    }

    pub fn models_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游模型列表",
            Locale::EnUS => "Upstream Models",
        }
    }
    pub fn models_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "查看与同步 DeepSeek 上游可用模型",
            Locale::EnUS => "View and sync available DeepSeek upstream models",
        }
    }
    pub fn models_sync_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "同步上游",
            Locale::EnUS => "Sync Upstream",
        }
    }
    pub fn models_syncing(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "同步中...",
            Locale::EnUS => "Syncing...",
        }
    }
    pub fn models_col_id(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型 ID",
            Locale::EnUS => "Model ID",
        }
    }
    pub fn models_col_owner(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "所有者",
            Locale::EnUS => "Owner",
        }
    }
    pub fn models_col_context(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上下文长度",
            Locale::EnUS => "Context Length",
        }
    }
    pub fn models_col_input_price(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输入价格",
            Locale::EnUS => "Input Price",
        }
    }
    pub fn models_col_output_price(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输出价格",
            Locale::EnUS => "Output Price",
        }
    }
    pub fn models_col_status(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "状态",
            Locale::EnUS => "Status",
        }
    }
    pub fn models_status_available(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "可用",
            Locale::EnUS => "Available",
        }
    }
    pub fn models_status_unavailable(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "不可用",
            Locale::EnUS => "Unavailable",
        }
    }
    pub fn models_synced_at(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上次同步",
            Locale::EnUS => "Last Synced",
        }
    }
    pub fn models_never_synced(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "从未同步",
            Locale::EnUS => "Never synced",
        }
    }
    pub fn models_empty(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "暂无模型数据，请点击「同步上游」获取",
            Locale::EnUS => "No models yet. Click 'Sync Upstream' to fetch.",
        }
    }
    pub fn models_sync_result(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "同步结果",
            Locale::EnUS => "Sync Result",
        }
    }
    pub fn models_added(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "新增",
            Locale::EnUS => "Added",
        }
    }
    pub fn models_removed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "移除",
            Locale::EnUS => "Removed",
        }
    }
    pub fn models_unchanged(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "不变",
            Locale::EnUS => "Unchanged",
        }
    }
    pub fn models_price_unit(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "/百万 Token",
            Locale::EnUS => "/M tokens",
        }
    }

    pub fn logs_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "可观测性与日志",
            Locale::EnUS => "Observability & Logs",
        }
    }
    pub fn logs_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "实时请求日志与追踪详情",
            Locale::EnUS => "Real-time request logs and trace details.",
        }
    }
    pub fn logs_refresh(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "刷新",
            Locale::EnUS => "Refresh",
        }
    }
    pub fn logs_col_time(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "时间",
            Locale::EnUS => "Time",
        }
    }
    pub fn logs_col_model(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型",
            Locale::EnUS => "Model",
        }
    }
    pub fn logs_col_consumer(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消费者",
            Locale::EnUS => "Consumer",
        }
    }
    pub fn logs_col_latency(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟",
            Locale::EnUS => "Latency",
        }
    }
    pub fn logs_col_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token",
            Locale::EnUS => "Tokens",
        }
    }
    pub fn logs_col_cache(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存",
            Locale::EnUS => "Cache",
        }
    }
    pub fn logs_details(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "详情",
            Locale::EnUS => "Details",
        }
    }
    pub fn logs_detail_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求详情",
            Locale::EnUS => "Request Details",
        }
    }
    pub fn logs_detail_timestamp(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "时间戳",
            Locale::EnUS => "Timestamp",
        }
    }
    pub fn logs_detail_model(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型",
            Locale::EnUS => "Model",
        }
    }
    pub fn logs_detail_consumer(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消费者",
            Locale::EnUS => "Consumer",
        }
    }
    pub fn logs_detail_latency(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟",
            Locale::EnUS => "Latency",
        }
    }
    pub fn logs_detail_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token",
            Locale::EnUS => "Tokens",
        }
    }
    pub fn logs_detail_cache_status(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存状态",
            Locale::EnUS => "Cache Status",
        }
    }
    pub fn logs_detail_payload(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求载荷",
            Locale::EnUS => "Request Payload",
        }
    }
    pub fn logs_detail_response(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "响应",
            Locale::EnUS => "Response",
        }
    }
    pub fn logs_empty(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "暂无请求日志",
            Locale::EnUS => "No request logs yet.",
        }
    }
    pub fn logs_load_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "加载日志失败",
            Locale::EnUS => "Failed to load logs",
        }
    }

    pub const fn empty_state_icon() -> &'static str { "—" }

    pub fn upstream_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游服务配置",
            Locale::EnUS => "Upstream Configuration",
        }
    }
    pub fn upstream_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "配置 CrabCache 连接上游 LLM API 服务的地址与认证密钥",
            Locale::EnUS => "Configure CrabCache upstream LLM API endpoint and authentication.",
        }
    }
    pub fn upstream_base_url_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 Base URL",
            Locale::EnUS => "Upstream Base URL",
        }
    }
    pub fn upstream_base_url_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "例如 https://api.deepseek.com",
            Locale::EnUS => "e.g. https://api.deepseek.com",
        }
    }
    pub fn upstream_api_key_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 API Key",
            Locale::EnUS => "Upstream API Key",
        }
    }
    pub fn upstream_api_key_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "用于向上游服务发起请求的密钥",
            Locale::EnUS => "The API key used to authenticate with the upstream service",
        }
    }
    pub fn upstream_api_key_masked_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已保存，留空则不修改",
            Locale::EnUS => "Saved, leave blank to keep unchanged",
        }
    }
    pub fn upstream_model_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "默认模型",
            Locale::EnUS => "Default Model",
        }
    }
    pub fn upstream_model_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求未指定模型时的默认回退模型",
            Locale::EnUS => "Fallback model when request does not specify one",
        }
    }
    pub fn upstream_endpoints_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游端点 (Ketama 环)",
            Locale::EnUS => "Upstream Endpoints (Ketama Ring)",
        }
    }
    pub fn upstream_endpoints_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "每行一个，格式 host:port",
            Locale::EnUS => "One per line, format host:port",
        }
    }
    pub fn upstream_save_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存配置",
            Locale::EnUS => "Save Config",
        }
    }
    pub fn upstream_saving(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存中...",
            Locale::EnUS => "Saving...",
        }
    }
    pub fn upstream_saved(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "配置已保存",
            Locale::EnUS => "Config saved",
        }
    }
    pub fn upstream_load_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "加载上游配置失败",
            Locale::EnUS => "Failed to load upstream config",
        }
    }
}

pub fn provide_locale() -> RwSignal<Locale> {
    let locale = RwSignal::new(Locale::ZhCN);
    provide_context(locale);
    locale
}

pub fn use_translations() -> Translations {
    let locale = use_context::<RwSignal<Locale>>()
        .expect("Locale context not found. Call provide_locale() first.");
    Translations {
        locale: locale.get(),
    }
}

pub fn use_locale() -> RwSignal<Locale> {
    use_context::<RwSignal<Locale>>()
        .expect("Locale context not found. Call provide_locale() first.")
}