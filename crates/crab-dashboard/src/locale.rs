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
            Locale::ZhCN => "多供应商 LLM API 网关 · DeepSeek V4",
            Locale::EnUS => "Multi-Provider LLM API Gateway · DeepSeek V4",
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
    pub fn sidebar_cache(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存",
            Locale::EnUS => "Cache",
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
    pub fn sidebar_requests(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求",
            Locale::EnUS => "Requests",
        }
    }
    pub fn tab_logs(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求日志",
            Locale::EnUS => "Logs",
        }
    }
    pub fn tab_insights(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "组成分析",
            Locale::EnUS => "Insights",
        }
    }
    pub fn tab_capture(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "包捕获",
            Locale::EnUS => "Capture",
        }
    }
    pub fn tab_log_manage(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "日志管理",
            Locale::EnUS => "Log Mgmt",
        }
    }
    pub fn capture_empty(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "暂无捕获记录",
            Locale::EnUS => "No captures yet",
        }
    }
    pub fn capture_empty_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "在网关配置启用 [raw_capture]（enabled = true），确认 Admin 的 CRABCACHE_RAW_CAPTURE_DIR 与网关 dir 一致后重启网关，再发送 chat 请求。"
            }
            Locale::EnUS => {
                "Enable [raw_capture] (enabled = true) in gateway config, align CRABCACHE_RAW_CAPTURE_DIR with the gateway dir, restart, then send a chat request."
            }
        }
    }
    pub fn capture_error_prefix(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "错误：",
            Locale::EnUS => "Error: ",
        }
    }
    pub fn capture_col_time(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "时间 (北京)",
            Locale::EnUS => "Time (CST)",
        }
    }
    pub fn capture_time_tz_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "北京时间",
            Locale::EnUS => "CST",
        }
    }
    pub fn capture_col_model(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型",
            Locale::EnUS => "Model",
        }
    }
    pub fn capture_col_consumer(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消费者",
            Locale::EnUS => "Consumer",
        }
    }
    pub fn capture_col_client(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "客户端",
            Locale::EnUS => "Client",
        }
    }
    pub fn capture_col_upstream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游",
            Locale::EnUS => "Upstream",
        }
    }
    pub fn capture_col_delta(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "增量",
            Locale::EnUS => "Delta",
        }
    }
    pub fn capture_col_msgs(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消息数",
            Locale::EnUS => "Msgs",
        }
    }
    pub fn capture_stat_total(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "捕获总数",
            Locale::EnUS => "Total Captures",
        }
    }
    pub fn capture_stat_avg_delta(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "平均增量",
            Locale::EnUS => "Avg Delta",
        }
    }
    pub fn capture_stat_reasoning_injected(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "推理注入率",
            Locale::EnUS => "Reasoning Injected",
        }
    }
    pub fn capture_stat_msg_p99(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消息数 P99",
            Locale::EnUS => "Msg Count P99",
        }
    }
    pub fn capture_stat_thinking_markup(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "思考标签率",
            Locale::EnUS => "Thinking Markup",
        }
    }
    pub fn capture_detail_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "捕获详情",
            Locale::EnUS => "Capture Detail",
        }
    }
    pub fn capture_no_body(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "未捕获正文",
            Locale::EnUS => "No body captured",
        }
    }
    pub fn capture_client_body(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "客户端请求体",
            Locale::EnUS => "Client Body",
        }
    }
    pub fn capture_upstream_body(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游请求体",
            Locale::EnUS => "Upstream Body",
        }
    }
    pub fn capture_structure_metric(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "指标",
            Locale::EnUS => "Metric",
        }
    }
    pub fn capture_structure_client(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "客户端",
            Locale::EnUS => "Client",
        }
    }
    pub fn capture_structure_upstream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游",
            Locale::EnUS => "Upstream",
        }
    }
    pub fn capture_structure_delta(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "差值",
            Locale::EnUS => "Delta",
        }
    }
    pub fn capture_row_messages(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消息条数",
            Locale::EnUS => "Messages",
        }
    }
    pub fn capture_row_content_chars(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "正文字符",
            Locale::EnUS => "Content Chars",
        }
    }
    pub fn capture_row_reasoning_chars(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "推理字符",
            Locale::EnUS => "Reasoning Chars",
        }
    }
    pub fn capture_row_system_chars(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "系统字符",
            Locale::EnUS => "System Chars",
        }
    }
    pub fn capture_row_tool_count(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "工具数",
            Locale::EnUS => "Tool Count",
        }
    }
    pub fn capture_row_thinking_markup(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "思考标签",
            Locale::EnUS => "Thinking Markup",
        }
    }
    pub fn capture_meta_stream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "流式",
            Locale::EnUS => "Stream",
        }
    }
    pub fn capture_meta_reasoning_strategy(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "推理策略",
            Locale::EnUS => "Reasoning Strategy",
        }
    }
    pub fn capture_meta_retired_prefix(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "退役前缀消息",
            Locale::EnUS => "Retired Prefix Msgs",
        }
    }
    pub fn capture_bool_yes(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "是",
            Locale::EnUS => "Yes",
        }
    }
    pub fn capture_bool_no(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "否",
            Locale::EnUS => "No",
        }
    }
    pub fn capture_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "包捕获",
            Locale::EnUS => "Packet Capture",
        }
    }
    pub fn capture_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "对比客户端与网关改写后的上游请求体，定位推理注入、思考标签与上下文膨胀。"
            }
            Locale::EnUS => {
                "Compare client vs gateway-rewritten upstream bodies to spot reasoning injection, thinking markup, and context growth."
            }
        }
    }
    pub fn capture_refresh(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "刷新",
            Locale::EnUS => "Refresh",
        }
    }
    pub fn capture_list_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "捕获列表",
            Locale::EnUS => "Capture list",
        }
    }
    pub fn capture_records_unit(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "条",
            Locale::EnUS => "records",
        }
    }
    pub fn capture_filter_consumer(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消费者",
            Locale::EnUS => "Consumer",
        }
    }
    pub fn capture_filter_project(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "项目 ID",
            Locale::EnUS => "Project ID",
        }
    }
    pub fn capture_filter_hash(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求哈希",
            Locale::EnUS => "Request hash",
        }
    }
    pub fn capture_filter_session(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "会话指纹",
            Locale::EnUS => "Session fingerprint",
        }
    }
    pub fn capture_filter_backend(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "后端节点",
            Locale::EnUS => "Backend",
        }
    }
    pub fn capture_col_session(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "会话",
            Locale::EnUS => "Session",
        }
    }
    pub fn capture_col_backend(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "后端",
            Locale::EnUS => "Backend",
        }
    }
    pub fn capture_col_duration(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "耗时 ms",
            Locale::EnUS => "ms",
        }
    }
    pub fn capture_filter_apply(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "筛选",
            Locale::EnUS => "Filter",
        }
    }
    pub fn capture_filter_clear(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "清除",
            Locale::EnUS => "Clear",
        }
    }
    pub fn capture_filter_active(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "（已筛选）",
            Locale::EnUS => "(filtered)",
        }
    }
    pub fn capture_select_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "点击左侧记录查看结构差异与请求体",
            Locale::EnUS => "Select a row to inspect structure diff and bodies",
        }
    }
    pub fn capture_section_structure(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "结构差异",
            Locale::EnUS => "Structure diff",
        }
    }
    pub fn capture_section_bodies(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求体",
            Locale::EnUS => "Bodies",
        }
    }
    pub fn capture_loading_detail(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "正在加载详情…",
            Locale::EnUS => "Loading detail...",
        }
    }
    pub fn capture_badge_reasoning(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "推理注入",
            Locale::EnUS => "Reasoning",
        }
    }
    pub fn capture_badge_thinking(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "思考标签",
            Locale::EnUS => "Thinking",
        }
    }
    pub fn capture_badge_sse(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "流式",
            Locale::EnUS => "SSE",
        }
    }
    pub fn capture_badge_large_delta(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "大包体",
            Locale::EnUS => "Large delta",
        }
    }
    pub fn tab_config(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "配置",
            Locale::EnUS => "Config",
        }
    }
    pub fn tab_routing(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "路由",
            Locale::EnUS => "Routing",
        }
    }
    pub fn tab_ops(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "运维",
            Locale::EnUS => "Ops",
        }
    }
    pub fn tab_trace(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "影子分析",
            Locale::EnUS => "Trace",
        }
    }
    pub fn tab_catalog(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型目录",
            Locale::EnUS => "Catalog",
        }
    }
    pub fn tab_aliases(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "别名映射",
            Locale::EnUS => "Aliases",
        }
    }
    pub fn tab_general(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "概览",
            Locale::EnUS => "General",
        }
    }
    pub fn tab_pipeline(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管道",
            Locale::EnUS => "Pipeline",
        }
    }
    pub fn tab_reasoning(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "推理",
            Locale::EnUS => "Reasoning",
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
            Locale::ZhCN => {
                "按 Consumer 查看端到端与上游 API 延迟、TTFT 及 Token 消耗（基于影子日志，约 5 秒粒度）"
            }
            Locale::EnUS => {
                "Per-consumer e2e vs upstream latency, TTFT, and token usage from shadow log (~5s buckets)"
            }
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
    pub fn live_window_1m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 1 分钟",
            Locale::EnUS => "Last 1 min",
        }
    }
    pub fn live_window_30m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 30 分钟",
            Locale::EnUS => "Last 30 min",
        }
    }
    pub fn live_window_10m(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 10 分钟",
            Locale::EnUS => "Last 10 min",
        }
    }
    pub fn live_window_1h(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 1 小时",
            Locale::EnUS => "Last 1 hour",
        }
    }
    pub fn live_window_12h(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 12 小时",
            Locale::EnUS => "Last 12 hours",
        }
    }
    pub fn live_window_1d(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 1 天",
            Locale::EnUS => "Last 1 day",
        }
    }
    pub fn live_window_3d(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 3 天",
            Locale::EnUS => "Last 3 days",
        }
    }
    pub fn live_window_7d(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 7 天",
            Locale::EnUS => "Last 7 days",
        }
    }
    pub fn live_window_15d(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 15 天",
            Locale::EnUS => "Last 15 days",
        }
    }
    pub fn live_window_30d(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "近 1 月",
            Locale::EnUS => "Last 30 days",
        }
    }
    pub fn live_cache_hit_trend(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存命中率趋势",
            Locale::EnUS => "Cache Hit Rate Trend",
        }
    }
    pub fn live_traffic_stats(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "流量统计",
            Locale::EnUS => "Traffic stats",
        }
    }
    pub fn live_config_consumer(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Consumer",
            Locale::EnUS => "Consumer",
        }
    }
    pub fn live_config_window(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "时间范围",
            Locale::EnUS => "Time range",
        }
    }
    pub fn live_config_refresh(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "刷新",
            Locale::EnUS => "Refresh",
        }
    }
    pub fn live_chart_throughput(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求吞吐",
            Locale::EnUS => "Request throughput",
        }
    }
    pub fn live_routing_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "负载均衡",
            Locale::EnUS => "Load balancing",
        }
    }
    pub fn live_routing_group(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "路由组",
            Locale::EnUS => "Routing group",
        }
    }
    pub fn live_nodes_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟明细",
            Locale::EnUS => "Latency detail",
        }
    }
    pub fn live_cache_hit_pct(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存命中",
            Locale::EnUS => "Cache hit",
        }
    }
    pub fn live_routing_loading(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "正在加载负载均衡状态…",
            Locale::EnUS => "Loading load-balancing status…",
        }
    }
    pub fn live_routing_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "负载均衡监控不可用",
            Locale::EnUS => "Load-balancing monitor unavailable",
        }
    }
    pub fn live_trace_unavailable(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "影子日志不可用。请在 gateway.toml 启用 trace_logging，并确保 Admin 能读取 trace.jsonl（Docker 需共享 gateway_logs 卷）。"
            }
            Locale::EnUS => {
                "Shadow log unavailable. Enable trace_logging in gateway.toml and mount trace.jsonl for Admin (gateway_logs volume in Docker)."
            }
        }
    }
    pub fn live_no_data(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "该时间窗内暂无此 Consumer 的请求数据",
            Locale::EnUS => "No requests for this consumer in the selected window",
        }
    }
    pub fn chart_click_to_expand(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "点击查看大图",
            Locale::EnUS => "Click for details",
        }
    }
    pub fn chart_detail_close(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "关闭",
            Locale::EnUS => "Close",
        }
    }
    pub fn sidebar_infra(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "基础设施",
            Locale::EnUS => "Infrastructure",
        }
    }
    pub fn infra_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "基础设施监控",
            Locale::EnUS => "Infrastructure",
        }
    }
    pub fn infra_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Docker 容器与宿主机资源占用（CPU、内存、网络、磁盘）",
            Locale::EnUS => "Docker container and host resource usage (CPU, memory, network, disk)",
        }
    }
    pub fn infra_download_speed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "下载速度",
            Locale::EnUS => "Download",
        }
    }
    pub fn infra_upload_speed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上传速度",
            Locale::EnUS => "Upload",
        }
    }
    pub fn infra_active_containers(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "活跃容器",
            Locale::EnUS => "Active",
        }
    }
    pub fn infra_mem_cumulative(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "内存占用",
            Locale::EnUS => "Memory used",
        }
    }
    pub fn infra_disk_cumulative(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "磁盘占用",
            Locale::EnUS => "Disk used",
        }
    }
    pub fn infra_docker_connected(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Docker 已连接",
            Locale::EnUS => "Docker OK",
        }
    }
    pub fn infra_expand_details(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "展开基础设施明细",
            Locale::EnUS => "Infrastructure details",
        }
    }
    pub fn infra_top_containers(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "容器 Top 3（按 CPU）",
            Locale::EnUS => "Top 3 containers by CPU",
        }
    }
    pub fn infra_containers(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "容器状态",
            Locale::EnUS => "Containers",
        }
    }
    pub fn infra_host_disk(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "宿主机磁盘",
            Locale::EnUS => "Host Disk",
        }
    }
    pub fn infra_container_name(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "名称",
            Locale::EnUS => "Name",
        }
    }
    pub fn infra_cpu(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "CPU",
            Locale::EnUS => "CPU",
        }
    }
    pub fn infra_memory(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "内存",
            Locale::EnUS => "Memory",
        }
    }
    pub fn infra_mem_usage(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "用量",
            Locale::EnUS => "Usage",
        }
    }
    pub fn infra_net_rx(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "下行",
            Locale::EnUS => "Down",
        }
    }
    pub fn infra_net_tx(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上行",
            Locale::EnUS => "Up",
        }
    }
    pub fn infra_status(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "状态",
            Locale::EnUS => "Status",
        }
    }
    pub fn infra_bps(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "B/s",
            Locale::EnUS => "B/s",
        }
    }
    pub fn infra_disk_usage(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "磁盘使用率",
            Locale::EnUS => "Disk Usage",
        }
    }
    pub fn infra_available(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "可用",
            Locale::EnUS => "Available",
        }
    }
    pub fn infra_docker_unavailable(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "Docker 不可用。请在 docker-compose.yml 中将 /var/run/docker.sock 挂载到 admin 容器，并设置 CRABCACHE_COMPOSE_PROJECT 环境变量。"
            }
            Locale::EnUS => {
                "Docker unavailable. Mount /var/run/docker.sock into the admin container and set CRABCACHE_COMPOSE_PROJECT."
            }
        }
    }
    pub fn infra_volumes(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "数据卷占用",
            Locale::EnUS => "Volumes",
        }
    }
    pub fn infra_volume_name(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "卷名",
            Locale::EnUS => "Volume",
        }
    }
    pub fn infra_history(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "历史趋势",
            Locale::EnUS => "History",
        }
    }
    pub fn infra_history_collecting(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "历史数据采集中（约 1 分钟后可见）",
            Locale::EnUS => "Collecting history (visible after ~1 min)",
        }
    }
    pub fn infra_speed_test(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "带宽测速",
            Locale::EnUS => "Speed test",
        }
    }
    pub fn infra_speed_test_run(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "开始测速（下载）",
            Locale::EnUS => "Run download test",
        }
    }
    pub fn infra_speed_test_running(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "测速进行中…",
            Locale::EnUS => "Test running…",
        }
    }
    pub fn infra_speed_test_result(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "下载",
            Locale::EnUS => "Download",
        }
    }
    pub fn infra_speed_test_upload(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上传测速",
            Locale::EnUS => "Upload test",
        }
    }
    pub fn infra_speed_test_both(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "双向测速",
            Locale::EnUS => "Download + upload",
        }
    }
    pub fn infra_speed_test_timeout(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "测速超时",
            Locale::EnUS => "Speed test timed out",
        }
    }
    pub fn infra_cpu_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "多核容器 CPU 可超过 100%",
            Locale::EnUS => "CPU may exceed 100% on multi-core containers",
        }
    }
    pub fn infra_compose_project(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Compose 项目",
            Locale::EnUS => "Compose project",
        }
    }
    pub fn infra_last_collected(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最近采集",
            Locale::EnUS => "Last collected",
        }
    }
    pub fn infra_history_samples(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "历史采样点",
            Locale::EnUS => "History samples",
        }
    }
    pub fn infra_select_container(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "图表容器",
            Locale::EnUS => "Chart container",
        }
    }
    pub fn infra_upload_result(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上传",
            Locale::EnUS => "Upload",
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
            Locale::ZhCN => "平均端到端延迟（含完整响应）",
            Locale::EnUS => "Avg e2e latency (full stream)",
        }
    }
    pub fn live_avg_upstream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游流式总时长（至 EOS）",
            Locale::EnUS => "Upstream stream duration (to EOS)",
        }
    }
    pub fn live_avg_ttft(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "平均首字 (TTFT)",
            Locale::EnUS => "Avg TTFT",
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
            Locale::ZhCN => {
                "上游延迟仅统计缓存未命中且已记录上游耗时的请求；缓存命中仅显示端到端延迟。流式 miss 下 E2E ≈ 上游属预期；可查看 TTFT 判断首字响应速度。"
            }
            Locale::EnUS => {
                "Upstream latency applies to cache misses with upstream timing; hits show e2e only. When streaming, e2e ≈ upstream is expected. Use TTFT to judge first-token responsiveness."
            }
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
            Locale::ZhCN => "端到端（完整流）",
            Locale::EnUS => "E2E (full stream)",
        }
    }
    pub fn live_series_upstream(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游流式",
            Locale::EnUS => "Upstream (full stream)",
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
    pub fn keys_rpm_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "0 表示不限速（推荐）。非 0 时网关按 sk-cc-* 每分钟请求数限流。",
            Locale::EnUS => {
                "0 = unlimited (recommended). Non-zero enforces per-key RPM on the gateway."
            }
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
    pub fn keys_col_rpm(self) -> &'static str {
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
            Locale::ZhCN => "完整密钥已保存，可在下方或列表中随时复制",
            Locale::EnUS => "Full key is saved; copy below or from the list anytime.",
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
            Locale::ZhCN => "Admin 未保存完整密钥（仅预览）",
            Locale::EnUS => "Full secret not stored in admin (preview only)",
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
    pub fn keys_col_concurrency(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "并发",
            Locale::EnUS => "Concurrency",
        }
    }
    pub fn keys_max_concurrent_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最大并发（0 = 不限制）",
            Locale::EnUS => "Max concurrent (0 = unlimited)",
        }
    }
    pub fn keys_concurrency_unlimited(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "∞",
            Locale::EnUS => "∞",
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
            Locale::EnUS => {
                "Global pipeline mode and default upstream profile; keys can override per client."
            }
        }
    }
    pub fn sidebar_system(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "系统",
            Locale::EnUS => "System",
        }
    }
    pub fn table_density_comfortable(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "舒适",
            Locale::EnUS => "Comfortable",
        }
    }
    pub fn table_density_compact(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "紧凑",
            Locale::EnUS => "Compact",
        }
    }
    pub fn session_monitor_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "检查每会话时间线和每密钥路由分布。",
            Locale::EnUS => "Inspect per-session timeline and per-key routing distribution.",
        }
    }
    pub fn session_monitor_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "会话监控",
            Locale::EnUS => "Session Monitor",
        }
    }
    pub fn session_timeline_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "会话时间线",
            Locale::EnUS => "Session Timeline",
        }
    }
    pub fn session_timeline_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "会话事件时间线（分钟桶）",
            Locale::EnUS => "Session events timeline (minute buckets)",
        }
    }
    pub fn audit_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "审计日志",
            Locale::EnUS => "Audit Log",
        }
    }
    pub fn audit_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管理操作审计轨迹",
            Locale::EnUS => "Management operation audit trail",
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
            Locale::EnUS => {
                "Are you sure you want to update? Services will restart briefly during the process."
            }
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
            Locale::ZhCN => "强制 Cursor 管线",
            Locale::EnUS => "Force Cursor pipeline",
        }
    }
    pub fn pipeline_mode_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "紧急调试：所有聊天走 V4 reasoning 管道；生产建议保持自动",
            Locale::EnUS => {
                "Emergency override: all chat uses V4 reasoning pipeline; use Auto in production."
            }
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
            Locale::ZhCN => "可在「上游」页管理 Profile；保存后立即生效",
            Locale::EnUS => "Manage profiles on the Upstream page; changes apply immediately.",
        }
    }
    pub fn upstream_profile_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 Profile",
            Locale::EnUS => "Upstream profile",
        }
    }
    pub fn upstream_provider_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "厂商 (provider)",
            Locale::EnUS => "Provider",
        }
    }
    pub fn upstream_new_profile_id(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "新建 Profile ID",
            Locale::EnUS => "New profile ID",
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
            Locale::EnUS => {
                "Per-domain quotas, hit-rate gates, and pipeline / upstream profile overrides."
            }
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
            Locale::ZhCN => {
                format!("指标环：{samples} 个采样点，最早约 {oldest_secs}s 前（已持久化到 SQLite）")
            }
            Locale::EnUS => format!(
                "Metrics ring: {samples} samples, oldest ~{oldest_secs}s ago (persisted via SQLite)"
            ),
        }
    }
    pub fn overview_gateway_reset(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "⚠ 检测到网关最近重启过，累计计数器已归零；历史曲线不受影响。",
            Locale::EnUS => {
                "⚠ Gateway was recently restarted; cumulative counters have reset but history curves are preserved."
            }
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
    pub fn overview_upstream_keys_hint(self, default_profile_id: &str) -> String {
        match self.locale {
            Locale::ZhCN => format!(
                "默认 Profile「{default_profile_id}」可用/已配置；其它 Profile 见上游配置页 Tab"
            ),
            Locale::EnUS => format!(
                "Default profile \"{default_profile_id}\" available/configured; see other profiles on Upstream page"
            ),
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
            Locale::EnUS => {
                "Are you sure you want to revoke the selected keys? This action cannot be undone."
            }
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
            Locale::EnUS => {
                "Are you sure you want to revoke this key? This action cannot be undone."
            }
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
            Locale::ZhCN => "管理推理处理管线",
            Locale::EnUS => "Manage reasoning pipeline",
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
            Locale::ZhCN => "多供应商 LLM API 高性能网关控制台 · DeepSeek V4",
            Locale::EnUS => "Control plane for the multi-provider LLM API gateway · DeepSeek V4",
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
            Locale::ZhCN => "查看与同步上游可用模型",
            Locale::EnUS => "View and sync available upstream models",
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
    pub fn logs_filter_model(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型",
            Locale::EnUS => "Model",
        }
    }
    pub fn logs_filter_consumer(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消费者",
            Locale::EnUS => "Consumer",
        }
    }
    pub fn logs_filter_cache_tier(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存层",
            Locale::EnUS => "Cache tier",
        }
    }
    pub fn logs_filter_hash(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求哈希",
            Locale::EnUS => "Request hash",
        }
    }
    pub fn logs_filter_latency_min(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟 ≥ ms",
            Locale::EnUS => "Latency ≥ ms",
        }
    }
    pub fn logs_filter_latency_max(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟 ≤ ms",
            Locale::EnUS => "Latency ≤ ms",
        }
    }
    pub fn logs_filter_token_min(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token ≥",
            Locale::EnUS => "Tokens ≥",
        }
    }
    pub fn logs_filter_token_max(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Token ≤",
            Locale::EnUS => "Tokens ≤",
        }
    }
    pub fn logs_filter_advanced(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "高级筛选",
            Locale::EnUS => "Advanced Filters",
        }
    }
    pub fn logs_filter_from_time(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "开始时间",
            Locale::EnUS => "From time",
        }
    }
    pub fn logs_filter_to_time(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "结束时间",
            Locale::EnUS => "To time",
        }
    }
    pub fn logs_latency_distribution(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟分布",
            Locale::EnUS => "Latency Distribution",
        }
    }
    pub fn logs_filter_apply(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "筛选",
            Locale::EnUS => "Filter",
        }
    }
    pub fn logs_filter_clear(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "清除",
            Locale::EnUS => "Clear",
        }
    }
    pub fn logs_filter_hash_click(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "点击按此哈希筛选列表",
            Locale::EnUS => "Click to filter list by this hash",
        }
    }
    pub fn logs_filter_cache_all(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "全部",
            Locale::EnUS => "All",
        }
    }
    pub fn logs_no_results(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无匹配日志",
            Locale::EnUS => "No matching logs.",
        }
    }
    pub fn logs_detail_diagnostics(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "诊断",
            Locale::EnUS => "Diagnostics",
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
    pub fn logs_col_input_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输入 T",
            Locale::EnUS => "In Tok",
        }
    }
    pub fn logs_col_output_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输出 T",
            Locale::EnUS => "Out Tok",
        }
    }
    pub fn logs_col_ttft(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "首字",
            Locale::EnUS => "TTFT",
        }
    }
    pub fn logs_col_len(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "长度",
            Locale::EnUS => "Len",
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
    pub fn logs_detail_project_id(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "project_id",
            Locale::EnUS => "project_id",
        }
    }
    pub fn logs_detail_upstream_user_id(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 user_id",
            Locale::EnUS => "Upstream user_id",
        }
    }
    pub fn logs_detail_user_id_audit(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "user_id 审计",
            Locale::EnUS => "user_id audit",
        }
    }
    pub fn logs_detail_latency(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟",
            Locale::EnUS => "Latency",
        }
    }
    pub fn logs_detail_latency_waterfall(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟瀑布图",
            Locale::EnUS => "Latency Waterfall",
        }
    }
    pub fn logs_detail_latency_waterfall_note(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "展示请求各阶段耗时：网关处理 → 上游响应 → 首字",
            Locale::EnUS => "Shows per-stage latency: Gateway processing → Upstream response → First token",
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
    pub fn logs_detail_upstream_latency(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游延迟",
            Locale::EnUS => "Upstream latency",
        }
    }
    pub fn logs_detail_ttft(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "首字延迟",
            Locale::EnUS => "TTFT",
        }
    }
    pub fn logs_detail_input_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输入 Token",
            Locale::EnUS => "Input tokens",
        }
    }
    pub fn logs_detail_output_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "输出 Token",
            Locale::EnUS => "Output tokens",
        }
    }
    pub fn logs_detail_request_hash(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求哈希",
            Locale::EnUS => "Request hash",
        }
    }
    pub fn logs_detail_semantic_cluster(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "语义簇",
            Locale::EnUS => "Semantic cluster",
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
            Locale::ZhCN => "预估命中率(上限)",
            Locale::EnUS => "Est. Hit Rate (upper bound)",
        }
    }
    pub fn trace_estimated_hit_rate_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "公式: repeat_ratio + (1 - repeat_ratio) * semantic_cluster_ratio。语义命中假设所有同 cluster 请求均命中 L2，实际受相似度阈值限制。"
            }
            Locale::EnUS => {
                "Formula: repeat_ratio + (1 - repeat_ratio) * semantic_cluster_ratio. Assumes all same-cluster requests hit L2; actual hit rate is bounded by similarity threshold."
            }
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
    pub fn trace_zipf_chart(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Zipf 分布 (log-log)",
            Locale::EnUS => "Zipf Distribution (log-log)",
        }
    }
    pub fn trace_deepseek_user_id_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 user_id 隔离审计",
            Locale::EnUS => "Upstream user_id isolation audit",
        }
    }
    pub fn trace_deepseek_user_id_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "对照 DeepSeek 官方：并发按账号计；user_id 用于 KVCache/调度隔离。injected 表示网关已将 project_id 写入上游 body.user_id。"
            }
            Locale::EnUS => {
                "Per DeepSeek docs: concurrency is per account; user_id isolates KV cache and scheduling. injected means project_id was written to upstream body.user_id."
            }
        }
    }
    pub fn trace_deepseek_requests(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游请求数",
            Locale::EnUS => "Upstream requests",
        }
    }
    pub fn trace_upstream_user_id_ratio(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 user_id 覆盖率",
            Locale::EnUS => "Upstream user_id coverage",
        }
    }
    pub fn trace_missing_project_id(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缺少 project_id",
            Locale::EnUS => "Missing project_id",
        }
    }
    pub fn trace_client_user_id_leaks(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "客户端 user_id 泄漏",
            Locale::EnUS => "Client user_id leaks",
        }
    }
    pub fn trace_audit_injected(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已注入 (injected)",
            Locale::EnUS => "Injected",
        }
    }
    pub fn trace_audit_absent(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缺失 (absent)",
            Locale::EnUS => "Absent",
        }
    }
    pub fn trace_top_project_ids(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 user_id (Top)",
            Locale::EnUS => "Upstream user_id (Top)",
        }
    }
    pub fn trace_isolation_ok(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "隔离状态：正常",
            Locale::EnUS => "Isolation: OK",
        }
    }
    pub fn trace_isolation_fail(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "隔离状态：需关注",
            Locale::EnUS => "Isolation: needs attention",
        }
    }

    // ── Composition page ──────────────────────────────────────────

    pub fn sidebar_composition(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求组成",
            Locale::EnUS => "Composition",
        }
    }

    pub fn composition_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请求组成分析",
            Locale::EnUS => "Request Composition Analysis",
        }
    }

    pub fn composition_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "分析请求的组成结构：模型分布、工具使用、Cursor 组件检测与消息数量",
            Locale::EnUS => {
                "Analyze request composition: model distribution, tool usage, Cursor component detection, and message counts"
            }
        }
    }

    pub fn composition_total_entries(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "总条目",
            Locale::EnUS => "Total Entries",
        }
    }

    pub fn composition_avg_latency(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "平均延迟",
            Locale::EnUS => "Avg Latency",
        }
    }

    pub fn composition_avg_tokens(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "平均 Token",
            Locale::EnUS => "Avg Tokens",
        }
    }

    pub fn composition_model_distribution(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型分布",
            Locale::EnUS => "Model Distribution",
        }
    }

    pub fn composition_tool_histogram(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "工具数量直方",
            Locale::EnUS => "Tool Count Histogram",
        }
    }

    pub fn composition_component_rates(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Cursor 组件检测率",
            Locale::EnUS => "Cursor Component Rates",
        }
    }

    pub fn composition_msg_histogram(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消息数量分布",
            Locale::EnUS => "Message Count Distribution",
        }
    }

    pub fn composition_project_distribution(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "项目分布",
            Locale::EnUS => "Project Distribution",
        }
    }

    pub fn composition_consumer_distribution(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消费者分布",
            Locale::EnUS => "Consumer Distribution",
        }
    }

    pub fn composition_trends(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "24h 请求趋势",
            Locale::EnUS => "24h Request Trend",
        }
    }

    pub fn composition_load_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "加载组成数据失败",
            Locale::EnUS => "Failed to load composition data",
        }
    }

    pub fn composition_no_data(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "暂无请求组成数据。请确认已启用 trace_logging 且存在带 composition 字段的日志条目。"
            }
            Locale::EnUS => {
                "No composition data yet. Ensure trace_logging is enabled and log entries contain composition fields."
            }
        }
    }

    pub fn composition_refresh(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "刷新",
            Locale::EnUS => "Refresh",
        }
    }

    pub fn composition_hours(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "小时",
            Locale::EnUS => "hours",
        }
    }

    pub fn composition_count(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "数量",
            Locale::EnUS => "Count",
        }
    }

    pub fn composition_tenant_count(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "租户数",
            Locale::EnUS => "Tenants",
        }
    }

    pub fn composition_consumer_count(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "消费者数",
            Locale::EnUS => "Consumers",
        }
    }

    pub fn composition_present(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "存在",
            Locale::EnUS => "Present",
        }
    }

    pub fn composition_rate(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "检测率",
            Locale::EnUS => "Detection Rate",
        }
    }

    pub fn composition_hourly_volume(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "小时请求量",
            Locale::EnUS => "Hourly Request Volume",
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
            Locale::ZhCN => "配置上游供应商的 relay 地址与 Key 池",
            Locale::EnUS => "Configure upstream provider relay and key pool.",
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
    pub fn upstream_pool_title_for(self, profile: &str) -> String {
        match self.locale {
            Locale::ZhCN => format!("{profile} 上游 Key 池"),
            Locale::EnUS => format!("{profile} upstream key pool"),
        }
    }
    pub fn upstream_pool_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "网关在未命中缓存时从当前 Profile 的 Key 池选取上游凭证。客户端请使用 sk-cc-*。"
            }
            Locale::EnUS => {
                "Gateway picks upstream credentials from this profile pool on cache miss. Clients use sk-cc-*."
            }
        }
    }
    pub fn upstream_pool_deepseek_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "DeepSeek：并发按账号计，与 Key 数量无关。多 Key 仅适用于多个 DeepSeek 账号；同账号多 Key 不提高并发。租户隔离请为 sk-cc-* 配置 project_id（上游 user_id）。"
            }
            Locale::EnUS => {
                "DeepSeek: concurrency is per account, not per API key. Use multiple keys only for multiple accounts. Bind project_id on sk-cc-* for upstream user_id isolation."
            }
        }
    }
    pub fn upstream_pool_patch_deepseek_only(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "仅默认 deepseek Profile 支持勾选启用；其它 Profile 请通过保存 Key 池更新。"
            }
            Locale::EnUS => {
                "Enable toggle only on the default deepseek profile; other profiles update via Save key pool."
            }
        }
    }
    pub fn upstream_pool_empty_keys_error(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "请至少输入一个上游 API Key（每行一个）",
            Locale::EnUS => "Enter at least one upstream API key (one per line)",
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
    pub fn upstream_pool_col_account(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "账号",
            Locale::EnUS => "Account",
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
    pub fn upstream_preset_custom(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "自定义中转",
            Locale::EnUS => "Custom relay",
        }
    }
    pub fn upstream_pick_template(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "选择预设模板",
            Locale::EnUS => "Pick a template",
        }
    }
    pub fn upstream_template_models_count(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "个模型可选",
            Locale::EnUS => "models available",
        }
    }
    pub fn upstream_model_custom(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "自定义模型...",
            Locale::EnUS => "Custom model...",
        }
    }
    pub fn upstream_back_to_templates(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "返回模板选择",
            Locale::EnUS => "Back to templates",
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
    pub fn upstream_tab_new_profile(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "+ 新建 Profile",
            Locale::EnUS => "+ New Profile",
        }
    }
    pub fn upstream_profile_delete_confirm(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "确定要删除此上游 Profile 吗？删除后其关联的 Key 池也会被吊销。",
            Locale::EnUS => {
                "Are you sure you want to delete this upstream profile? Its associated key pool will also be revoked."
            }
        }
    }
    pub fn upstream_test_latency(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "连接延迟",
            Locale::EnUS => "Connection latency",
        }
    }
    pub fn upstream_test_status_code(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "连接状态",
            Locale::EnUS => "Connection status",
        }
    }
    pub fn upstream_test_models_found(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "模型数量",
            Locale::EnUS => "Model count",
        }
    }
    pub fn upstream_key_status_enabled(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "启用",
            Locale::EnUS => "Enabled",
        }
    }
    pub fn upstream_key_status_disabled(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "停用",
            Locale::EnUS => "Disabled",
        }
    }
    pub fn upstream_test_profile_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "测试 Profile",
            Locale::EnUS => "Test Profile",
        }
    }
    pub fn upstream_test_all_quotas(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "批量测试额度",
            Locale::EnUS => "Test All Quotas",
        }
    }
    pub fn upstream_testing_all(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "测试中...",
            Locale::EnUS => "Testing...",
        }
    }
    pub fn upstream_clear_results(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "清除结果",
            Locale::EnUS => "Clear Results",
        }
    }
    pub fn upstream_pool_col_quota(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "额度",
            Locale::EnUS => "Quota",
        }
    }
    pub fn upstream_pool_col_test(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "测试",
            Locale::EnUS => "Test",
        }
    }
    pub fn upstream_quota_available(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "可用",
            Locale::EnUS => "OK",
        }
    }
    pub fn upstream_quota_exhausted(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "不足",
            Locale::EnUS => "Low",
        }
    }
    pub fn upstream_quota_na(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "无数据",
            Locale::EnUS => "N/A",
        }
    }
    pub fn upstream_quota_test_failed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "测试失败",
            Locale::EnUS => "Failed",
        }
    }
    pub fn upstream_quota_auth_failed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "认证失败",
            Locale::EnUS => "Auth Failed",
        }
    }
    pub fn upstream_quota_rate_limited(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "限流",
            Locale::EnUS => "Rate Limited",
        }
    }
    pub fn upstream_connection_test_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "连接测试",
            Locale::EnUS => "Connection Test",
        }
    }
    pub fn upstream_delete_profile_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "删除 Profile",
            Locale::EnUS => "Delete Profile",
        }
    }
    pub fn upstream_tls_sni_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "TLS SNI 域名",
            Locale::EnUS => "TLS SNI Domain",
        }
    }
    pub fn upstream_tls_sni_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游 TLS 握手的 SNI 服务器名称（可选）",
            Locale::EnUS => "SNI Server Name Indication for upstream TLS handshake (optional)",
        }
    }
    pub fn upstream_subtab_profiles(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Profiles",
            Locale::EnUS => "Profiles",
        }
    }
    pub fn upstream_subtab_routing(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "路由与健康",
            Locale::EnUS => "Routing & Health",
        }
    }
    pub fn upstream_subtab_keys(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "Key 池",
            Locale::EnUS => "Key Pool",
        }
    }
    pub fn routing_backend_name(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "名称",
            Locale::EnUS => "Name",
        }
    }
    pub fn routing_backend_addr(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "地址",
            Locale::EnUS => "Address",
        }
    }
    pub fn routing_backend_weight(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "权重",
            Locale::EnUS => "Weight",
        }
    }
    pub fn routing_backend_health(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "健康",
            Locale::EnUS => "Health",
        }
    }
    pub fn routing_backend_circuit(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "熔断",
            Locale::EnUS => "Circuit",
        }
    }
    pub fn routing_backend_failures(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "连续失败",
            Locale::EnUS => "Failures",
        }
    }
    pub fn routing_circuit_closed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "正常",
            Locale::EnUS => "Closed",
        }
    }
    pub fn routing_circuit_open(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "熔断",
            Locale::EnUS => "Open",
        }
    }
    pub fn routing_circuit_half_open(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "半开",
            Locale::EnUS => "HalfOpen",
        }
    }
    pub fn routing_key_pool_summary(self, available: usize, total: usize) -> String {
        match self.locale {
            Locale::ZhCN => format!("Key 池: {}/{} 可用", available, total),
            Locale::EnUS => format!("Key Pool: {}/{} available", available, total),
        }
    }
    pub fn routing_circuit_breaker_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "熔断参数",
            Locale::EnUS => "Circuit Breaker Config",
        }
    }
    pub fn routing_circuit_breaker_desc(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "以下为只读展示，修改请编辑 gateway.toml",
            Locale::EnUS => "Read-only display; edit gateway.toml to change",
        }
    }
    pub fn routing_no_backends(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "暂无后端节点，请先在 Profiles Tab 配置 endpoints",
            Locale::EnUS => "No backends configured. Add endpoints in the Profiles tab.",
        }
    }
    pub fn routing_manage_keys_link(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "管理 Key 池 →",
            Locale::EnUS => "Manage Key Pool →",
        }
    }
    pub fn routing_health_healthy(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "健康",
            Locale::EnUS => "Healthy",
        }
    }
    pub fn routing_health_unhealthy(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "不健康",
            Locale::EnUS => "Unhealthy",
        }
    }
    pub fn routing_last_check(self, ms_ago: u64) -> String {
        match self.locale {
            Locale::ZhCN => format!("{}s 前", ms_ago),
            Locale::EnUS => format!("{}s ago", ms_ago),
        }
    }
    pub fn routing_latency_ms(self, ms: u64) -> String {
        format!("{}ms", ms)
    }
    pub fn routing_summary_card_title(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "路由健康",
            Locale::EnUS => "Routing Health",
        }
    }
    pub fn routing_summary_healthy_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "健康节点",
            Locale::EnUS => "Healthy Backends",
        }
    }
    pub fn routing_summary_circuit_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "熔断中",
            Locale::EnUS => "Circuit Open",
        }
    }

    pub fn overview_setup_upstream_cta(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "尚未配置上游 Key 池，请前往上游配置。",
            Locale::EnUS => "Upstream key pool is empty. Configure upstream.",
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

    pub fn overview_tab_status(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "状态",
            Locale::EnUS => "Status",
        }
    }

    pub fn overview_tab_analytics(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "分析",
            Locale::EnUS => "Analytics",
        }
    }

    pub fn overview_module_cache_cost(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "缓存与成本",
            Locale::EnUS => "Cache & Cost",
        }
    }
    pub fn overview_module_consumer_domain(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "租户与域",
            Locale::EnUS => "Consumers & Domains",
        }
    }
    pub fn overview_module_latency_ops(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "延迟与运维",
            Locale::EnUS => "Latency & Ops",
        }
    }
    pub fn overview_module_advanced(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "高级指标",
            Locale::EnUS => "Advanced",
        }
    }

    pub fn anomaly_hit_rate_drop(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "命中率骤降：从 {:.1}% 降至 {:.1}%",
            Locale::EnUS => "Hit rate dropped: {:.1}% to {:.1}%",
        }
    }

    pub fn anomaly_qps_spike(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "QPS 突增：从 {:.1} 升至 {:.1}",
            Locale::EnUS => "QPS spiked: {:.1} to {:.1}",
        }
    }

    pub fn anomaly_latency_spike(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "上游延迟飙升：{:.0}ms",
            Locale::EnUS => "Upstream latency spike: {:.0}ms",
        }
    }

    pub fn anomaly_health_failed(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "网关健康检查失败",
            Locale::EnUS => "Gateway health check failed",
        }
    }

    pub fn anomaly_health_recovered(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "网关已恢复",
            Locale::EnUS => "Gateway recovered",
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
            Locale::ZhCN => "project_id（上游 user_id）",
            Locale::EnUS => "project_id (upstream user_id)",
        }
    }
    pub fn keys_project_id_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => {
                "强烈建议填写：网关会覆写上游 body.user_id，实现官方 KVCache/调度隔离。格式 [a-zA-Z0-9\\-_]+，最长 512。"
            }
            Locale::EnUS => {
                "Recommended: gateway overwrites upstream body.user_id for official KV cache and scheduling isolation. Format [a-zA-Z0-9\\-_]+, max 512."
            }
        }
    }
    pub fn keys_upstream_profile_hint(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "留空则按模型自动选择上游 Profile；多租户场景请同时配置 project_id。",
            Locale::EnUS => {
                "Leave empty for auto profile by model; for multi-tenant also set project_id."
            }
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

    // --- Logs Manage page ---
    pub fn logs_manage_disk_usage(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "磁盘使用",
            Locale::EnUS => "Disk Usage",
        }
    }
    pub fn logs_manage_trace_logs(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "追踪日志",
            Locale::EnUS => "Trace Logs",
        }
    }
    pub fn logs_manage_debug_trace(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "调试追踪",
            Locale::EnUS => "Debug Trace",
        }
    }
    pub fn logs_manage_capture_index(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "捕获索引",
            Locale::EnUS => "Capture Index",
        }
    }
    pub fn logs_manage_capture_bodies(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "捕获正文",
            Locale::EnUS => "Capture Bodies",
        }
    }
    pub fn logs_manage_files(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "个文件",
            Locale::EnUS => "files",
        }
    }
    pub fn logs_manage_retention_policy(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保留策略",
            Locale::EnUS => "Retention Policy",
        }
    }
    pub fn logs_manage_auto_cleanup(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "每 10 分钟自动清理",
            Locale::EnUS => "Auto-cleanup every 10 min",
        }
    }
    pub fn logs_manage_max_age_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最大保留时长 (小时，0 = 禁用)",
            Locale::EnUS => "Max Age (hours, 0 = disabled)",
        }
    }
    pub fn logs_manage_max_disk_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最大磁盘占用 (MB，0 = 禁用)",
            Locale::EnUS => "Max Disk (MB, 0 = disabled)",
        }
    }
    pub fn logs_manage_max_trace_files(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最大轮转追踪文件数",
            Locale::EnUS => "Max Rotated Trace Files",
        }
    }
    pub fn logs_manage_max_capture_files(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "最大捕获正文文件数",
            Locale::EnUS => "Max Capture Body Files",
        }
    }
    pub fn logs_manage_retention_saved(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保留策略已保存。",
            Locale::EnUS => "Retention policy saved.",
        }
    }
    pub fn logs_manage_manual_cleanup(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "手动清理",
            Locale::EnUS => "Manual Cleanup",
        }
    }
    pub fn logs_manage_target_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "目标",
            Locale::EnUS => "Target",
        }
    }
    pub fn logs_manage_target_all(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "全部（活跃文件除外）",
            Locale::EnUS => "All (except active files)",
        }
    }
    pub fn logs_manage_target_trace(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "追踪日志（仅轮转）",
            Locale::EnUS => "Trace Logs (rotated only)",
        }
    }
    pub fn logs_manage_target_debug(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "调试追踪（仅轮转）",
            Locale::EnUS => "Debug Trace (rotated only)",
        }
    }
    pub fn logs_manage_target_capture(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "捕获（索引 + 正文）",
            Locale::EnUS => "Capture (index + bodies)",
        }
    }
    pub fn logs_manage_older_than_label(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "仅早于（小时，留空 = 全部）",
            Locale::EnUS => "Only older than (hours, empty = all)",
        }
    }
    pub fn logs_manage_confirm_msg(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "确认操作？此操作不可撤销。",
            Locale::EnUS => "Are you sure? This cannot be undone.",
        }
    }
    pub fn logs_manage_clearing(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "清理中...",
            Locale::EnUS => "Clearing...",
        }
    }
    pub fn logs_manage_clear_btn(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "清理日志",
            Locale::EnUS => "Clear Logs",
        }
    }
    pub fn logs_manage_save_policy(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存策略",
            Locale::EnUS => "Save Policy",
        }
    }
    pub fn logs_manage_save_policy_saving(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "保存中…",
            Locale::EnUS => "Saving…",
        }
    }
    pub fn logs_manage_older_than_placeholder(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "例如：24",
            Locale::EnUS => "e.g. 24",
        }
    }
    pub fn logs_manage_cleared_fmt(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "已清理 {} 个文件，释放 {}",
            Locale::EnUS => "Cleared {} files, freed {}",
        }
    }
    pub fn logs_manage_error_fmt(self) -> &'static str {
        match self.locale {
            Locale::ZhCN => "错误：{}",
            Locale::EnUS => "Error: {}",
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
