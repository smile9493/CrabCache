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
    pub const fn sidebar_brand() -> &'static str {
        "CrabCache"
    }
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
    pub fn sidebar_cache_ops(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存运维",
            Locale::EnUS => "Cache Ops",
        }
    }
    pub fn sidebar_models(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型列表",
            Locale::EnUS => "Models",
        }
    }
    pub fn sidebar_reasoning(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "推理管线",
            Locale::EnUS => "Reasoning",
        }
    }
    pub fn sidebar_cursor_models(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Cursor 别名",
            Locale::EnUS => "Cursor Aliases",
        }
    }
    pub fn sidebar_logs(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "日志与追踪",
            Locale::EnUS => "Logs & Traces",
        }
    }
    pub fn sidebar_live(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "实时监控",
            Locale::EnUS => "Live",
        }
    }
    pub fn live_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "客户端实时速度",
            Locale::EnUS => "Live client speed",
        }
    }
    pub fn live_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "按 Consumer 查看端到端与上游 API 延迟、TTFT 及 Token 消耗（基于影子日志，约 5 秒粒度）",
            Locale::EnUS => "Per-consumer e2e vs upstream latency, TTFT, and token usage from shadow log (~5s buckets)",
        }
    }
    pub fn live_auto_refresh(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "自动刷新 (2s)",
            Locale::EnUS => "Auto-refresh (2s)",
        }
    }
    pub fn live_consumer_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Consumer（客户端）",
            Locale::EnUS => "Consumer (client)",
        }
    }
    pub fn live_select_consumer(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "选择 Consumer…",
            Locale::EnUS => "Select consumer…",
        }
    }
    pub fn live_pick_consumer_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请从上方下拉框选择一个 Consumer，以查看实时指标",
            Locale::EnUS => "Select a consumer above to view live metrics",
        }
    }
    pub fn live_keys_load_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无法加载 Consumer 列表",
            Locale::EnUS => "Failed to load consumer list",
        }
    }
    pub fn live_window_5m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 5 分钟",
            Locale::EnUS => "Last 5 min",
        }
    }
    pub fn live_window_15m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 15 分钟",
            Locale::EnUS => "Last 15 min",
        }
    }
    pub fn live_trace_unavailable(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "影子日志不可用。请在 gateway.toml 启用 trace_logging，并确保 Admin 能读取 trace.jsonl（Docker 需共享 gateway_logs 卷）。",
            Locale::EnUS => "Shadow log unavailable. Enable trace_logging in gateway.toml and mount trace.jsonl for Admin (gateway_logs volume in Docker).",
        }
    }
    pub fn live_no_data(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "该时间窗内暂无此 Consumer 的请求数据",
            Locale::EnUS => "No requests for this consumer in the selected window",
        }
    }
    pub fn live_requests(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求数",
            Locale::EnUS => "Requests",
        }
    }
    pub fn live_qps(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近窗 QPS",
            Locale::EnUS => "Window QPS",
        }
    }
    pub fn live_upstream_na(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "—",
            Locale::EnUS => "—",
        }
    }
    pub fn live_avg_e2e(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "平均端到端延迟",
            Locale::EnUS => "Avg e2e latency",
        }
    }
    pub fn live_avg_upstream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "平均上游延迟",
            Locale::EnUS => "Avg upstream latency",
        }
    }
    pub fn live_tokens_total(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token 合计",
            Locale::EnUS => "Total tokens",
        }
    }
    pub fn live_tokens_in_out(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输入 / 输出",
            Locale::EnUS => "Input / output",
        }
    }
    pub fn live_latency_chart(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟趋势",
            Locale::EnUS => "Latency trend",
        }
    }
    pub fn live_upstream_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游延迟仅统计缓存未命中且已记录上游耗时的请求；缓存命中仅显示端到端延迟。",
            Locale::EnUS => "Upstream latency applies to cache misses with upstream timing; hits show e2e only.",
        }
    }
    pub fn live_token_chart(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token 消耗趋势",
            Locale::EnUS => "Token usage trend",
        }
    }
    pub fn live_series_e2e(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "端到端",
            Locale::EnUS => "E2E",
        }
    }
    pub fn live_series_upstream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游",
            Locale::EnUS => "Upstream",
        }
    }
    pub fn live_series_ttft(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "首字 (TTFT)",
            Locale::EnUS => "TTFT",
        }
    }
    pub fn live_tokens_input(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输入 Token",
            Locale::EnUS => "Input tokens",
        }
    }
    pub fn live_tokens_output(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输出 Token",
            Locale::EnUS => "Output tokens",
        }
    }
    pub fn live_latest_request(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最近一次请求",
            Locale::EnUS => "Latest request",
        }
    }
    pub fn live_latest_model(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型",
            Locale::EnUS => "Model",
        }
    }
    pub fn live_latest_cache(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存状态",
            Locale::EnUS => "Cache status",
        }
    }
    pub fn sidebar_upstream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游配置",
            Locale::EnUS => "Upstream",
        }
    }
    pub fn sidebar_trace(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "影子日志",
            Locale::EnUS => "Shadow Logs",
        }
    }
    pub fn sidebar_online(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "网关运行中",
            Locale::EnUS => "Gateway Online",
        }
    }
    pub fn sidebar_gateway_checking(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "检查网关…",
            Locale::EnUS => "Checking gateway…",
        }
    }
    pub fn sidebar_gateway_offline(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "网关不可达",
            Locale::EnUS => "Gateway offline",
        }
    }
    pub fn sidebar_group_monitor(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "监控",
            Locale::EnUS => "Monitor",
        }
    }
    pub fn sidebar_group_config(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "配置",
            Locale::EnUS => "Configure",
        }
    }
    pub fn sidebar_group_ops(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "运维",
            Locale::EnUS => "Operations",
        }
    }

    pub fn theme(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "切换主题",
            Locale::EnUS => "Theme",
        }
    }

    pub const fn not_found_title() -> &'static str {
        "404"
    }
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
    pub const fn overview_qps() -> &'static str {
        "QPS"
    }
    pub fn overview_qps_sub(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "每秒请求数",
            Locale::EnUS => "Requests per second",
        }
    }
    pub const fn overview_tps() -> &'static str {
        "TPS"
    }
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
    pub const fn overview_l0_label() -> &'static str {
        "L0 (Moka)"
    }
    pub const fn overview_l1_label() -> &'static str {
        "L1 (Redis)"
    }
    pub const fn overview_l2_label() -> &'static str {
        "L2 (Semantic)"
    }
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
    pub const fn overview_latency_l0() -> &'static str {
        "L0 Moka"
    }
    pub const fn overview_latency_l1() -> &'static str {
        "L1 Redis"
    }
    pub const fn overview_latency_l2() -> &'static str {
        "L2 Qdrant"
    }
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
    pub fn overview_error_hint_502(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "502：检查 crab-admin 是否在运行，OpenResty proxy_pass 是否指向 127.0.0.1:18001（非 8080）。"
            }
            Locale::EnUS => {
                "502: ensure crab-admin is running and OpenResty proxy_pass targets 127.0.0.1:18001 (not 8080)."
            }
        }
    }
    pub fn overview_error_hint_503(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "503：检查网关 :9090/metrics 与 CRABCACHE_GATEWAY_METRICS_URL，确认 gateway 容器健康。"
            }
            Locale::EnUS => {
                "503: check gateway :9090/metrics and CRABCACHE_GATEWAY_METRICS_URL; verify gateway is healthy."
            }
        }
    }
    pub fn overview_suggestions_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "运维建议",
            Locale::EnUS => "Suggestions",
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
    pub const fn keys_col_rpm() -> &'static str {
        "RPM"
    }
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
    pub fn keys_gateway_url_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "网关地址",
            Locale::EnUS => "Gateway URL",
        }
    }
    pub fn keys_gateway_url_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "客户端请求应发送到此地址",
            Locale::EnUS => "Clients should send requests to this address",
        }
    }
    pub fn keys_copy_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "复制",
            Locale::EnUS => "Copy",
        }
    }
    pub fn keys_created_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥已创建",
            Locale::EnUS => "Key Created",
        }
    }
    pub fn keys_created_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请立即复制此密钥，关闭后将无法再次查看完整密钥",
            Locale::EnUS => "Copy this key now. You won't be able to see it again.",
        }
    }
    pub fn keys_created_done(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "完成",
            Locale::EnUS => "Done",
        }
    }
    pub fn keys_copy_ok(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已复制到剪贴板",
            Locale::EnUS => "Copied to clipboard",
        }
    }
    pub fn keys_copy_failed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "复制失败",
            Locale::EnUS => "Copy failed",
        }
    }
    pub fn keys_copy_unavailable(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "完整密钥仅在创建时可见",
            Locale::EnUS => "Full key only visible at creation",
        }
    }
    pub fn keys_network_load_failed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无法检测网关地址，请配置 CRABCACHE_GATEWAY_CLIENT_LAN_HOST 等环境变量",
            Locale::EnUS => {
                "Could not detect gateway URLs; set CRABCACHE_GATEWAY_CLIENT_LAN_HOST or related env vars"
            }
        }
    }
    pub fn keys_copy_config_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "复制配置",
            Locale::EnUS => "Copy config",
        }
    }
    pub fn keys_unlimited_quota(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无限配额",
            Locale::EnUS => "Unlimited Quota",
        }
    }
    pub fn keys_quota_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token 配额",
            Locale::EnUS => "Token Quota",
        }
    }
    pub fn keys_col_quota(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "配额",
            Locale::EnUS => "Quota",
        }
    }
    pub fn keys_unlimited(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无限",
            Locale::EnUS => "Unlimited",
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

    pub fn sidebar_pipeline(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求管道",
            Locale::EnUS => "Pipelines",
        }
    }
    pub fn pipeline_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求管道",
            Locale::EnUS => "Request Pipelines",
        }
    }
    pub fn pipeline_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "全局管道模式、默认上游 profile；Key 可单独覆盖 pipeline / profile",
            Locale::EnUS => "Global pipeline mode and default upstream profile; keys can override per client.",
        }
    }
    pub fn sidebar_system(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "系统",
            Locale::EnUS => "System",
        }
    }
    pub fn system_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "系统管理",
            Locale::EnUS => "System",
        }
    }
    pub fn system_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "版本更新、Admin 密钥管理",
            Locale::EnUS => "Binary updates and admin key management.",
        }
    }
    pub fn system_version_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "版本与更新",
            Locale::EnUS => "Version & Updates",
        }
    }
    pub fn system_current_version(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "当前版本",
            Locale::EnUS => "Current Version",
        }
    }
    pub fn system_latest_version(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最新版本",
            Locale::EnUS => "Latest Version",
        }
    }
    pub fn system_check_updates(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "检查更新",
            Locale::EnUS => "Check for Updates",
        }
    }
    pub fn system_update_now(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "立即更新",
            Locale::EnUS => "Update Now",
        }
    }
    pub fn system_update_confirm(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "确认要更新到最新版本吗？更新过程中服务会短暂重启。",
            Locale::EnUS => "Are you sure you want to update? Services will restart briefly during the process.",
        }
    }
    pub fn system_update_available(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "有新版本可用",
            Locale::EnUS => "Update available",
        }
    }
    pub fn system_up_to_date(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已是最新版本",
            Locale::EnUS => "Up to date",
        }
    }
    pub fn system_checking(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "检查中...",
            Locale::EnUS => "Checking...",
        }
    }
    pub fn system_admin_key_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Admin 密钥管理",
            Locale::EnUS => "Admin Key Management",
        }
    }
    pub fn system_admin_key_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "修改后立即生效，下次登录需使用新密钥。",
            Locale::EnUS => "Takes effect immediately. Use the new key for your next login.",
        }
    }
    pub fn system_current_key(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "当前密钥",
            Locale::EnUS => "Current Key",
        }
    }
    pub fn system_new_key(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "新密钥",
            Locale::EnUS => "New Key",
        }
    }
    pub fn system_confirm_key(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "确认新密钥",
            Locale::EnUS => "Confirm New Key",
        }
    }
    pub fn system_change_key(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "更改密钥",
            Locale::EnUS => "Change Key",
        }
    }
    pub fn system_key_mismatch(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "新密钥与确认密码不一致",
            Locale::EnUS => "New keys do not match",
        }
    }
    pub fn system_key_too_short(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "新密钥至少 4 个字符",
            Locale::EnUS => "New key must be at least 4 characters",
        }
    }
    pub fn system_key_changed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥已更新，请使用新密钥重新登录",
            Locale::EnUS => "Key updated. Please re-login with the new key.",
        }
    }
    pub fn system_key_change_failed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥修改失败",
            Locale::EnUS => "Failed to change key",
        }
    }
    pub fn system_published(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "发布时间",
            Locale::EnUS => "Published",
        }
    }
    pub fn system_no_release(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无法获取 GitHub 版本信息",
            Locale::EnUS => "Unable to fetch GitHub release info",
        }
    }
    pub fn pipeline_mode_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "全局管道模式",
            Locale::EnUS => "Global pipeline mode",
        }
    }
    pub fn pipeline_mode_auto(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "自动（按模型与客户端信号）",
            Locale::EnUS => "Auto (model + client signals)",
        }
    }
    pub fn pipeline_mode_force(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "强制 Cursor DeepSeek V4",
            Locale::EnUS => "Force Cursor DeepSeek V4",
        }
    }
    pub fn pipeline_mode_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "紧急调试：所有聊天走 V4 reasoning 管道；生产建议保持自动",
            Locale::EnUS => "Emergency override: all chat uses V4 reasoning pipeline; use Auto in production.",
        }
    }
    pub fn pipeline_default_profile(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "默认上游 Profile",
            Locale::EnUS => "Default upstream profile",
        }
    }
    pub fn pipeline_profiles_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已注册 Profile",
            Locale::EnUS => "Registered profiles",
        }
    }
    pub fn pipeline_provider_col(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "厂商",
            Locale::EnUS => "Provider",
        }
    }
    pub fn pipeline_profiles_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Profile 在 gateway.toml 的 [[upstream.profiles]] 中定义；修改后需重启网关",
            Locale::EnUS => "Profiles are defined in gateway.toml [[upstream.profiles]]; restart gateway after file changes.",
        }
    }
    pub fn pipeline_save(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存",
            Locale::EnUS => "Save",
        }
    }
    pub fn pipeline_saving(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存中…",
            Locale::EnUS => "Saving…",
        }
    }
    pub fn pipeline_save_ok(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管道配置已更新",
            Locale::EnUS => "Pipeline settings updated",
        }
    }
    pub fn keys_pipeline_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管道覆盖",
            Locale::EnUS => "Pipeline override",
        }
    }
    pub fn keys_upstream_profile_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 Profile",
            Locale::EnUS => "Upstream profile",
        }
    }
    pub fn keys_override_auto(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "自动",
            Locale::EnUS => "Auto",
        }
    }

    pub fn cache_ops_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存运维",
            Locale::EnUS => "Cache Operations",
        }
    }
    pub fn cache_ops_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管理指纹版本、流式缓存开关与缓存失效",
            Locale::EnUS => "Manage fingerprint version, stream cache, and cache invalidation.",
        }
    }
    pub fn cache_ops_fingerprint_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存键指纹",
            Locale::EnUS => "Cache Key Fingerprint",
        }
    }
    pub fn cache_ops_fingerprint_version(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "指纹版本",
            Locale::EnUS => "Fingerprint version",
        }
    }
    pub fn cache_ops_normalize(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "内容规范化 (NFC/空白)",
            Locale::EnUS => "Normalize content (NFC/whitespace)",
        }
    }
    pub fn cache_ops_stream_cache(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "流式响应缓存",
            Locale::EnUS => "Stream response cache",
        }
    }
    pub fn cache_ops_invalidate_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存失效",
            Locale::EnUS => "Cache invalidation",
        }
    }
    pub fn cache_ops_scope(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "范围 (all / prefix:ns / key)",
            Locale::EnUS => "Scope (all / prefix:ns / key)",
        }
    }
    pub fn cache_ops_invalidate_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "执行失效",
            Locale::EnUS => "Invalidate",
        }
    }
    pub fn cache_ops_confirm_all_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "确认清空全部缓存？",
            Locale::EnUS => "Invalidate entire cache?",
        }
    }
    pub fn cache_ops_confirm_all_body(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "将清空 L0 与 L1（Redis SCAN）。此操作异步执行，状态为 accepted 表示已受理。"
            }
            Locale::EnUS => {
                "Clears L0 and L1 (Redis SCAN). Runs asynchronously; accepted means the job was queued."
            }
        }
    }
    pub fn cache_ops_confirm_ok(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "确认",
            Locale::EnUS => "Confirm",
        }
    }
    pub fn cache_ops_confirm_cancel(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "取消",
            Locale::EnUS => "Cancel",
        }
    }
    pub fn cache_ops_last_invalidate(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上次失效",
            Locale::EnUS => "Last invalidation",
        }
    }
    pub fn cache_ops_none(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无",
            Locale::EnUS => "None",
        }
    }
    pub fn cache_ops_invalidate_running(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "全量失效进行中…",
            Locale::EnUS => "Full invalidation in progress…",
        }
    }
    pub fn cache_ops_invalidate_job(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Gateway 异步任务",
            Locale::EnUS => "Gateway async job",
        }
    }
    pub fn overview_semantic_guard(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "语义守卫",
            Locale::EnUS => "Semantic guard",
        }
    }
    pub fn overview_semantic_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "命中 / 拒绝 / 跳过（与 L2 tier 命中不同）",
            Locale::EnUS => "hit / rejected / skipped (not L2 tier hits)",
        }
    }
    pub fn overview_hit_rate_5m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "命中率 (5m)",
            Locale::EnUS => "Hit rate (5m)",
        }
    }
    pub fn overview_token_hit_rate_5m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token 命中率 (5m)",
            Locale::EnUS => "Token hit rate (5m)",
        }
    }
    pub fn overview_qps_5m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "QPS (5m)",
            Locale::EnUS => "QPS (5m)",
        }
    }
    pub fn overview_hit_rate_cumulative_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "自启动累计",
            Locale::EnUS => "Since process start",
        }
    }
    pub fn overview_sample_insufficient(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "样本不足（等待指标采样，约 1–2 分钟）",
            Locale::EnUS => "Insufficient samples (wait ~1–2 min for metrics sampling)",
        }
    }
    pub fn overview_collecting_timeseries(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "时序采集中（每分钟采样，请稍候）",
            Locale::EnUS => "Collecting time series (samples every minute)",
        }
    }
    pub fn overview_gateway_cache_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "网关缓存 L0–L2",
            Locale::EnUS => "Gateway cache L0–L2",
        }
    }
    pub fn overview_coalescing_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求合并 (Coalescing)",
            Locale::EnUS => "Request coalescing",
        }
    }
    pub fn overview_coalescing_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "并发相同缓存键时合并为一次上游调用",
            Locale::EnUS => "Duplicate concurrent keys merged into one upstream call",
        }
    }
    pub fn overview_semantic_card_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "语义缓存守卫",
            Locale::EnUS => "Semantic cache guard",
        }
    }
    pub fn overview_consumer_table_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "按 Consumer (API Key)",
            Locale::EnUS => "By consumer (API key)",
        }
    }
    pub fn overview_consumer_col(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Consumer",
            Locale::EnUS => "Consumer",
        }
    }
    pub fn sidebar_domains(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "领域",
            Locale::EnUS => "Domains",
        }
    }
    pub fn domains_page_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "领域缓存分析",
            Locale::EnUS => "Domain cache analytics",
        }
    }
    pub fn domains_page_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "按业务领域聚合的 Token 命中率、成本节省与 QPS。",
            Locale::EnUS => "Token hit rate, cost saved, and QPS grouped by business domain.",
        }
    }
    pub fn domains_table_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "领域缓存总览",
            Locale::EnUS => "Domain cache overview",
        }
    }
    pub fn domains_compare_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "多项目命中率对比",
            Locale::EnUS => "Cross-domain hit rate",
        }
    }
    pub fn domains_col_domain(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "领域",
            Locale::EnUS => "Domain",
        }
    }
    pub fn domains_col_hit_rate(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "命中率",
            Locale::EnUS => "Hit rate",
        }
    }
    pub fn domains_col_cost(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "节省成本",
            Locale::EnUS => "Cost saved",
        }
    }
    pub fn domains_col_qps(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "QPS (5m)",
            Locale::EnUS => "QPS (5m)",
        }
    }
    pub fn domains_col_alert(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "告警",
            Locale::EnUS => "Alert",
        }
    }
    pub fn domains_view_all(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "查看全部",
            Locale::EnUS => "View all",
        }
    }
    pub fn domains_back(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "← 返回领域列表",
            Locale::EnUS => "← Back to domains",
        }
    }
    pub fn domains_hit_rate(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token 命中率",
            Locale::EnUS => "Token hit rate",
        }
    }
    pub fn domains_cost_saved(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "累计节省",
            Locale::EnUS => "Cost saved",
        }
    }
    pub fn domains_tier_breakdown(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存层级 (5m)",
            Locale::EnUS => "Cache tiers (5m)",
        }
    }
    pub fn domains_policies_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "域名策略",
            Locale::EnUS => "Domain policies",
        }
    }
    pub fn domains_policies_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "按域名设置配额、命中率门槛，以及管道 / 上游 profile 覆盖",
            Locale::EnUS => "Per-domain quotas, hit-rate gates, and pipeline / upstream profile overrides.",
        }
    }
    pub fn domains_policy_edit_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "编辑域名策略",
            Locale::EnUS => "Edit domain policy",
        }
    }
    pub fn domains_policy_add(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "添加策略",
            Locale::EnUS => "Add policy",
        }
    }
    pub fn domains_policy_save(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存策略",
            Locale::EnUS => "Save policy",
        }
    }
    pub fn domains_policy_delete(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "删除策略",
            Locale::EnUS => "Delete policy",
        }
    }
    pub fn domains_policy_saved(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "域名策略已保存并同步到网关",
            Locale::EnUS => "Domain policy saved and synced to gateway",
        }
    }
    pub fn domains_policy_deleted(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "域名策略已删除",
            Locale::EnUS => "Domain policy deleted",
        }
    }
    pub fn domains_policy_col_pipeline(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管道",
            Locale::EnUS => "Pipeline",
        }
    }
    pub fn domains_policy_col_profile(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Profile",
            Locale::EnUS => "Profile",
        }
    }
    pub fn domains_policy_budget_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "月 Token 预算",
            Locale::EnUS => "Monthly token budget",
        }
    }
    pub fn domains_policy_budget_cost(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "月成本预算 (USD)",
            Locale::EnUS => "Monthly cost budget (USD)",
        }
    }
    pub fn domains_policy_min_hit_rate(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最低命中率",
            Locale::EnUS => "Min hit rate",
        }
    }
    pub fn domains_policy_enabled(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "启用策略",
            Locale::EnUS => "Policy enabled",
        }
    }
    pub fn domains_policy_new_domain(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "域名标识",
            Locale::EnUS => "Domain id",
        }
    }
    pub fn keys_domain_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "领域 (可选)",
            Locale::EnUS => "Domain (optional)",
        }
    }
    pub fn overview_trend_hit_rate(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "命中率",
            Locale::EnUS => "Hit rate",
        }
    }
    pub fn trace_hours_note(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "影子日志近 24h 实测命中率；Overview 5m 来自 Prometheus 采样，二者口径不同。"
            }
            Locale::EnUS => {
                "Shadow log hit rate (last 24h). Overview 5m uses Prometheus sampling; different windows."
            }
        }
    }
    pub fn overview_cache_hits(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存命中",
            Locale::EnUS => "Cache hits",
        }
    }
    pub fn overview_cache_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存 Token",
            Locale::EnUS => "Cache tokens",
        }
    }
    pub fn overview_prefix_cache_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游前缀缓存 (L3)",
            Locale::EnUS => "Upstream prefix cache (L3)",
        }
    }
    pub fn overview_prefix_cache_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "DeepSeek prompt_cache_hit_tokens / (hit+miss)。与 L0/L1/L2 网关响应缓存无关。"
            }
            Locale::EnUS => {
                "DeepSeek prompt_cache_hit_tokens / (hit+miss). Separate from L0/L1/L2 gateway response cache."
            }
        }
    }
    pub fn overview_legend_l0_l2(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "L0–L2：网关整响应缓存（精确 + 语义 tier）",
            Locale::EnUS => "L0–L2: gateway full-response cache (exact + semantic tier)",
        }
    }
    pub fn overview_legend_l3(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "L3：上游 DeepSeek 前缀 token 缓存（每次请求均计量）",
            Locale::EnUS => "L3: upstream DeepSeek prefix token cache (counted on every request)",
        }
    }
    pub fn overview_legend_5m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "主数字 5m：Prometheus 采样环滑动窗口",
            Locale::EnUS => "Primary numbers (5m): sliding window from Prometheus sampling ring",
        }
    }
    pub fn overview_legend_cumulative(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "累计：自网关进程启动的 counter 总和",
            Locale::EnUS => "Cumulative: counter totals since gateway process start",
        }
    }
    pub fn overview_health_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "网关健康",
            Locale::EnUS => "Gateway health",
        }
    }
    pub fn overview_health_upstream_keys(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 Key 可用",
            Locale::EnUS => "Upstream keys available",
        }
    }
    pub fn overview_health_stream_cache(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "流式响应缓存",
            Locale::EnUS => "Stream response cache",
        }
    }
    pub fn overview_health_unhealthy(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "网关不可达",
            Locale::EnUS => "Gateway unreachable",
        }
    }
    pub fn overview_prefix_by_model(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "按模型",
            Locale::EnUS => "By model",
        }
    }
    pub fn overview_cost_saved_total(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "累计节省 (Prometheus)",
            Locale::EnUS => "Total saved (Prometheus)",
        }
    }
    pub fn overview_cost_saved_5m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 5 分钟节省",
            Locale::EnUS => "Saved (last 5m)",
        }
    }
    pub fn overview_cost_pricing_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "来自 gateway_cache_cost_saved_usd_total；定价见网关配置",
            Locale::EnUS => "From gateway_cache_cost_saved_usd_total; pricing in gateway config",
        }
    }
    pub fn overview_semantic_disabled(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "语义缓存已关闭",
            Locale::EnUS => "Semantic cache disabled",
        }
    }
    pub fn overview_ops_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "运维指标",
            Locale::EnUS => "Operations",
        }
    }
    pub fn overview_ops_ttft(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "首字延迟 (TTFT)",
            Locale::EnUS => "Time to first token (TTFT)",
        }
    }
    pub fn overview_ops_coalesced_5m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "合并 (5m)",
            Locale::EnUS => "Coalesced (5m)",
        }
    }
    pub fn overview_ops_rejected_5m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "拒绝 (5m)",
            Locale::EnUS => "Rejected (5m)",
        }
    }
    pub fn overview_trace_compare_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "影子日志 vs 网关 5m",
            Locale::EnUS => "Shadow log vs gateway 5m",
        }
    }
    pub fn overview_trace_compare_link(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "打开 Trace 分析",
            Locale::EnUS => "Open trace analysis",
        }
    }
    pub fn overview_history_meta(self, samples: usize, oldest_secs: u64) -> String {
        match self.locale {
            Locale::ZhCN => format!(
                "指标环：{samples} 个采样点，最早约 {oldest_secs}s 前（已持久化到 SQLite）"
            ),
            Locale::EnUS => format!(
                "Metrics ring: {samples} samples, oldest ~{oldest_secs}s ago (persisted via SQLite)"
            ),
        }
    }
    pub fn overview_gateway_reset(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "⚠ 检测到网关最近重启过，累计计数器已归零；历史曲线不受影响。",
            Locale::EnUS => "⚠ Gateway was recently restarted; cumulative counters have reset but history curves are preserved.",
        }
    }
    pub fn overview_l3_input_ratio(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "L3 占 input tokens 比例",
            Locale::EnUS => "L3 share of input tokens",
        }
    }
    pub fn overview_timeseries_1h(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "1 小时",
            Locale::EnUS => "1 hour",
        }
    }
    pub fn overview_timeseries_24h(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "24 小时",
            Locale::EnUS => "24 hours",
        }
    }
    pub fn overview_timeseries_7d(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "7 天",
            Locale::EnUS => "7 days",
        }
    }
    pub fn overview_upstream_keys_strip(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 Key 池",
            Locale::EnUS => "Upstream key pool",
        }
    }
    pub fn overview_upstream_keys_link(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管理上游",
            Locale::EnUS => "Manage upstream",
        }
    }
    pub fn overview_prefix_health_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "前缀与 Reasoning 保护",
            Locale::EnUS => "Prefix & reasoning protection",
        }
    }
    pub fn overview_prefix_health_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "prefix_break 与 reasoning 存储 miss 上升时，L3 命中率可能下降。恢复策略见 Reasoning 配置。"
            }
            Locale::EnUS => {
                "Rising prefix_break or reasoning store misses can lower L3 hit rate. See Reasoning settings for recovery."
            }
        }
    }
    pub fn overview_observability_doc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "可观测性文档",
            Locale::EnUS => "Observability docs",
        }
    }
    pub fn overview_tier_5m_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 5 分钟分层命中（请求数）",
            Locale::EnUS => "Tier hits in last 5m (request count)",
        }
    }
    pub fn upstream_l3_affinity_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "Ketama + x-conversation-id / x-prompt-cache-key 将同一会话固定到同一上游 peer，提升 L3 命中率。详见 docs/DEEPSEEK_PREFIX_CACHE.md"
            }
            Locale::EnUS => {
                "Ketama plus x-conversation-id / x-prompt-cache-key stick sessions to one upstream peer for higher L3 hit rate. See docs/DEEPSEEK_PREFIX_CACHE.md"
            }
        }
    }
    pub fn overview_token_hit(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "命中",
            Locale::EnUS => "Hit",
        }
    }
    pub fn overview_token_miss(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "未命中",
            Locale::EnUS => "Miss",
        }
    }
    pub fn overview_status_active(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "运行中",
            Locale::EnUS => "Active",
        }
    }
    pub fn keys_lan_url_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "局域网地址",
            Locale::EnUS => "LAN URL",
        }
    }
    pub fn keys_openresty_url_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "OpenResty 地址",
            Locale::EnUS => "OpenResty URL",
        }
    }
    pub fn cache_ops_scope_required(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请填写失效范围",
            Locale::EnUS => "Scope is required",
        }
    }
    pub fn cache_ops_ttl_config_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存 TTL 配置",
            Locale::EnUS => "Cache TTL Config",
        }
    }
    pub fn cache_ops_l0_ttl(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "L0 TTL (秒)",
            Locale::EnUS => "L0 TTL (s)",
        }
    }
    pub fn cache_ops_l1_ttl(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "L1 TTL (秒)",
            Locale::EnUS => "L1 TTL (s)",
        }
    }
    pub fn cache_ops_model_overrides(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型 TTL 覆盖",
            Locale::EnUS => "Model TTL Overrides",
        }
    }
    pub fn cache_ops_consumer_overrides(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消费者 TTL 覆盖",
            Locale::EnUS => "Consumer TTL Overrides",
        }
    }
    pub fn cache_ops_add_override(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "添加",
            Locale::EnUS => "Add",
        }
    }
    pub fn cache_ops_key_placeholder(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "名称 (例如 deepseek-chat)",
            Locale::EnUS => "Name (e.g. deepseek-chat)",
        }
    }
    pub fn cache_ops_remove(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "删除",
            Locale::EnUS => "Remove",
        }
    }
    pub fn keys_batch_revoke_body(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "确定批量吊销所选密钥？此操作不可撤销。",
            Locale::EnUS => "Are you sure you want to revoke the selected keys? This action cannot be undone.",
        }
    }
    pub fn keys_batch_revoke_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "批量吊销密钥",
            Locale::EnUS => "Batch Revoke Keys",
        }
    }
    pub fn keys_confirm_revoke_body(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "确定吊销此密钥？此操作不可撤销。",
            Locale::EnUS => "Are you sure you want to revoke this key? This action cannot be undone.",
        }
    }
    pub fn cache_ops_semantic_enabled(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "启用语义缓存",
            Locale::EnUS => "Enable Semantic Cache",
        }
    }
    pub fn cache_ops_semantic_threshold(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "相似度阈值",
            Locale::EnUS => "Similarity Threshold",
        }
    }
    pub fn reasoning_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Reasoning 配置",
            Locale::EnUS => "Reasoning Config",
        }
    }
    pub fn reasoning_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管理 DeepSeek 推理处理管线",
            Locale::EnUS => "Manage DeepSeek reasoning pipeline",
        }
    }
    pub fn reasoning_thinking_mode(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "思考模式",
            Locale::EnUS => "Thinking Mode",
        }
    }
    pub fn reasoning_effort(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "推理力度",
            Locale::EnUS => "Reasoning Effort",
        }
    }
    pub fn reasoning_recovery(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "推理恢复",
            Locale::EnUS => "Reasoning Recovery",
        }
    }
    pub fn reasoning_sqlite_cache(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "SQLite 缓存",
            Locale::EnUS => "SQLite Cache",
        }
    }
    pub fn reasoning_sqlite_path(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存路径",
            Locale::EnUS => "Cache Path",
        }
    }
    pub fn cursor_models_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Cursor 模型别名",
            Locale::EnUS => "Cursor Model Aliases",
        }
    }
    pub fn cursor_models_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管理 Cursor 模型别名映射",
            Locale::EnUS => "Manage Cursor model alias map",
        }
    }
    pub fn cursor_models_col_model(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型",
            Locale::EnUS => "Model",
        }
    }
    pub fn cursor_models_col_alias(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "别名",
            Locale::EnUS => "Alias",
        }
    }
    pub fn cursor_models_alias_add(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "添加别名",
            Locale::EnUS => "Add Alias",
        }
    }
    pub fn auth_tagline(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "DeepSeek V4 高性能 API 网关控制台",
            Locale::EnUS => "Control plane for the DeepSeek V4 API gateway",
        }
    }

    pub fn auth_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Admin 登录",
            Locale::EnUS => "Admin Sign In",
        }
    }
    pub fn auth_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请输入与服务器 CRABCACHE_ADMIN_KEY 一致的密钥。",
            Locale::EnUS => "Enter the key matching the server CRABCACHE_ADMIN_KEY.",
        }
    }
    pub fn auth_key_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Admin API Key",
            Locale::EnUS => "Admin API Key",
        }
    }
    pub fn auth_key_placeholder(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "x-admin-key",
            Locale::EnUS => "x-admin-key",
        }
    }
    pub fn auth_submit(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "登录",
            Locale::EnUS => "Sign In",
        }
    }
    pub fn auth_dev_default(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "使用开发默认密钥 (admin)",
            Locale::EnUS => "Use development default (admin)",
        }
    }
    pub fn auth_error_empty(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请输入 Admin API Key",
            Locale::EnUS => "Admin API Key is required",
        }
    }
    pub fn auth_error_save(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无法保存密钥（localStorage 不可用）",
            Locale::EnUS => "Failed to save key (localStorage unavailable)",
        }
    }
    pub fn auth_error_invalid(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥无效，请确认与服务器 CRABCACHE_ADMIN_KEY 一致",
            Locale::EnUS => "Invalid key — must match the server CRABCACHE_ADMIN_KEY",
        }
    }
    pub fn auth_verifying(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "验证中…",
            Locale::EnUS => "Verifying…",
        }
    }
    pub fn sidebar_change_admin_key(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "更改 Admin 密钥",
            Locale::EnUS => "Change Admin Key",
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
    pub fn logs_detail_route(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "路由后端",
            Locale::EnUS => "Route backend",
        }
    }
    pub fn logs_detail_cache_path(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存路径",
            Locale::EnUS => "Cache path",
        }
    }
    pub fn logs_select_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "选择一条请求查看详情",
            Locale::EnUS => "Select a request to view details",
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
    pub fn logs_load_more(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "加载更多",
            Locale::EnUS => "Load more",
        }
    }
    pub fn logs_loading(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "加载中…",
            Locale::EnUS => "Loading…",
        }
    }

    pub fn trace_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "影子日志分析",
            Locale::EnUS => "Shadow Log Analysis",
        }
    }
    pub fn trace_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "安全采集的生产数据统计特征，用于缓存命中率评估和参数调优",
            Locale::EnUS => "Privacy-preserving production data analysis for cache optimization",
        }
    }
    pub fn trace_refresh(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "刷新分析",
            Locale::EnUS => "Refresh Analysis",
        }
    }
    pub fn trace_load_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "加载分析数据失败",
            Locale::EnUS => "Failed to load analysis",
        }
    }
    pub fn trace_total_requests(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "总请求数",
            Locale::EnUS => "Total Requests",
        }
    }
    pub fn trace_unique_requests(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "唯一请求数",
            Locale::EnUS => "Unique Requests",
        }
    }
    pub fn trace_repeat_ratio(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "重复率",
            Locale::EnUS => "Repeat Ratio",
        }
    }
    pub fn trace_estimated_hit_rate(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "预估命中率",
            Locale::EnUS => "Est. Hit Rate",
        }
    }
    pub fn trace_semantic_ratio(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "语义聚类率",
            Locale::EnUS => "Semantic Cluster Ratio",
        }
    }
    pub fn trace_zipf_alpha(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Zipf 参数 α",
            Locale::EnUS => "Zipf Alpha",
        }
    }
    pub fn trace_cache_hit_ratio(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "实际缓存命中率",
            Locale::EnUS => "Actual Cache Hit Ratio",
        }
    }
    pub fn trace_avg_metrics(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "平均指标",
            Locale::EnUS => "Average Metrics",
        }
    }
    pub fn trace_avg_latency(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "平均延迟",
            Locale::EnUS => "Avg Latency",
        }
    }
    pub fn trace_avg_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "平均 Token 数",
            Locale::EnUS => "Avg Tokens",
        }
    }
    pub fn trace_top_models(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "热门模型",
            Locale::EnUS => "Top Models",
        }
    }
    pub fn trace_cluster_distribution(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "聚类分布",
            Locale::EnUS => "Cluster Distribution",
        }
    }

    pub const fn empty_state_icon() -> &'static str {
        "—"
    }

    pub fn upstream_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游服务配置",
            Locale::EnUS => "Upstream Configuration",
        }
    }
    pub fn upstream_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "配置 DeepSeek 官方或 OpenAI 兼容中转的 relay 地址与 Key 池",
            Locale::EnUS => "Configure DeepSeek official or OpenAI-compatible relay and key pool.",
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
            Locale::ZhCN => "仅用于 Dashboard「同步模型列表」；网关转发请使用下方 Key 池",
            Locale::EnUS => "For Dashboard model sync only; gateway proxy uses the key pool below",
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
    pub fn upstream_pool_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "DeepSeek 上游 Key 池",
            Locale::EnUS => "DeepSeek upstream key pool",
        }
    }
    pub fn upstream_pool_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "配额聚合：网关在缓存未命中时轮换使用以下 Key。客户端请使用 sk-cc-*，勿使用 DeepSeek Key。"
            }
            Locale::EnUS => {
                "Quota pool: gateway rotates these keys on cache miss. Clients must use sk-cc-* keys, not DeepSeek keys."
            }
        }
    }
    pub fn upstream_pool_col_id(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "ID",
            Locale::EnUS => "ID",
        }
    }
    pub fn upstream_pool_col_preview(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "预览",
            Locale::EnUS => "Preview",
        }
    }
    pub fn upstream_pool_col_enabled(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "启用",
            Locale::EnUS => "Enabled",
        }
    }
    pub fn upstream_pool_col_inflight(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "进行中",
            Locale::EnUS => "Inflight",
        }
    }
    pub fn upstream_pool_col_cooldown(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "冷却 (秒)",
            Locale::EnUS => "Cooldown (s)",
        }
    }
    pub fn upstream_pool_replace_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "替换 Key 池（每行一个 secret）",
            Locale::EnUS => "Replace key pool (one secret per line)",
        }
    }
    pub fn upstream_pool_save_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存 Key 池",
            Locale::EnUS => "Save key pool",
        }
    }
    pub fn upstream_pool_saving(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存中...",
            Locale::EnUS => "Saving...",
        }
    }
    pub fn upstream_pool_saved(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已保存",
            Locale::EnUS => "Saved",
        }
    }
    pub fn upstream_test_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "测试连接",
            Locale::EnUS => "Test connection",
        }
    }
    pub fn upstream_testing(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "测试中...",
            Locale::EnUS => "Testing...",
        }
    }
    pub fn upstream_preset_official(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "DeepSeek 官方",
            Locale::EnUS => "DeepSeek official",
        }
    }
    pub fn upstream_preset_custom(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "自定义中转",
            Locale::EnUS => "Custom relay",
        }
    }
    pub fn upstream_show_advanced(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "显示高级选项（Ketama 端点）",
            Locale::EnUS => "Show advanced (Ketama endpoints)",
        }
    }
    pub fn upstream_hide_advanced(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "隐藏高级选项",
            Locale::EnUS => "Hide advanced",
        }
    }
    pub fn upstream_gateway_unreachable(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无法连接网关管理 API，显示的是上次保存的配置快照。",
            Locale::EnUS => "Gateway management API unreachable; showing last saved snapshot.",
        }
    }
    pub fn upstream_pool_append_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "追加 Key（每行一个 secret）",
            Locale::EnUS => "Append keys (one secret per line)",
        }
    }
    pub fn upstream_pool_replace_confirm(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "替换整个 Key 池（取消勾选则为追加）",
            Locale::EnUS => "Replace entire key pool (uncheck to append)",
        }
    }
    pub fn overview_setup_upstream_cta(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "尚未配置 DeepSeek 上游 Key 池，请前往上游配置。",
            Locale::EnUS => "DeepSeek upstream key pool is empty. Configure upstream.",
        }
    }
    pub fn overview_setup_upstream_link(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "配置上游",
            Locale::EnUS => "Configure upstream",
        }
    }
    pub fn models_detect_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "检测差异",
            Locale::EnUS => "Detect changes",
        }
    }
    pub fn models_apply_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "应用所选",
            Locale::EnUS => "Apply selected",
        }
    }
    pub fn models_detect_result(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型差异预览",
            Locale::EnUS => "Model diff preview",
        }
    }

    pub fn overview_token_stats(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token 统计",
            Locale::EnUS => "Token Statistics",
        }
    }

    pub fn overview_input_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输入 Token",
            Locale::EnUS => "Input Tokens",
        }
    }

    pub fn overview_output_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输出 Token",
            Locale::EnUS => "Output Tokens",
        }
    }

    pub fn overview_total_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "总 Token",
            Locale::EnUS => "Total Tokens",
        }
    }

    pub fn overview_usage_trends(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "使用趋势",
            Locale::EnUS => "Usage Trends",
        }
    }

    pub fn overview_hourly(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "小时",
            Locale::EnUS => "Hourly",
        }
    }

    pub fn overview_daily(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "日",
            Locale::EnUS => "Daily",
        }
    }

    pub fn overview_weekly(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "周",
            Locale::EnUS => "Weekly",
        }
    }

    pub fn overview_monthly(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "月",
            Locale::EnUS => "Monthly",
        }
    }

    pub fn overview_no_data(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "此时间范围暂无数据",
            Locale::EnUS => "No data available for this time range",
        }
    }

    pub fn overview_requests(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求",
            Locale::EnUS => "req",
        }
    }

    pub fn overview_hits(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "命中",
            Locale::EnUS => "hits",
        }
    }

    pub fn overview_auto_refresh(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "自动刷新",
            Locale::EnUS => "Auto refresh",
        }
    }

    pub fn overview_last_update(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最后更新",
            Locale::EnUS => "Last update",
        }
    }

    pub fn overview_refresh(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "刷新",
            Locale::EnUS => "Refresh",
        }
    }

    pub fn keys_search_placeholder(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "搜索密钥...",
            Locale::EnUS => "Search keys...",
        }
    }

    pub fn keys_no_results(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "没有匹配的密钥",
            Locale::EnUS => "No keys match your search",
        }
    }
    pub fn keys_project_id_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "项目 ID",
            Locale::EnUS => "Project ID",
        }
    }
    pub fn keys_edit_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "编辑",
            Locale::EnUS => "Edit",
        }
    }
    pub fn keys_edit_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "编辑密钥",
            Locale::EnUS => "Edit Key",
        }
    }
    pub fn keys_save_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存",
            Locale::EnUS => "Save",
        }
    }
    pub fn keys_edit_success(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥更新成功",
            Locale::EnUS => "Key updated successfully",
        }
    }
    pub fn keys_edit_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "密钥更新失败",
            Locale::EnUS => "Failed to update key",
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
