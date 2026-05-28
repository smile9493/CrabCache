# CrabCache UI 设计规范

Admin Dashboard（Leptos WASM）与 [demo.html](demo.html) 共用同一套设计令牌。机器可读真源：[crates/crab-dashboard/style/design-tokens.css](../crates/crab-dashboard/style/design-tokens.css)。

## 1. 产品注册表

| 项 | 说明 |
|----|------|
| Register | Product（运维工具 / 可观测面板） |
| 默认主题 | Dark |
| 使用场景 | 运维在暗光环境长时间盯屏，快速扫读指标与日志 |
| 色彩策略 | Restrained：中性底 + 珊瑚 accent ≤10% 面积 |
| 禁止 | accent 大面积渐变背景；虚构命中率；混谈 L0–L2 与 L3 指标 |

## 2. 品牌

- **Logo**：[`favicon.svg`](../crates/crab-dashboard/style/favicon.svg)（侧栏与登录页，组件 `BrandLogo`）
- **标题渐变**：`linear-gradient(135deg, --cc-accent, --cc-yellow)`（类名 `.brand-gradient-text`）
- **产品名**：CrabCache

## 3. 设计令牌

语义前缀 `--cc-*`。五套主题（`theme-dark` / `theme-light` / `theme-midnight` / `theme-ocean` / `theme-sand`）必须包含**相同键名**。

| Token | 用途 | Dark | Light | Midnight | Ocean | Sand |
|-------|------|------|-------|----------|-------|------|
| `--cc-bg` | 主内容区背景 | `#0d1117` | `#f6f4f0` | `#0c0a14` | `#0a1218` | `#f4f0e8` |
| `--cc-bg-sidebar` | 侧栏/顶栏背景 | `#0a0e13` | `#ebe8e2` | `#080610` | `#070e14` | `#ebe5da` |
| `--cc-bg-card` | 卡片/面板 | `#161b22` | `#ffffff` | `#14101f` | `#111c26` | `#faf7f2` |
| `--cc-bg-elevated` | 悬停/下拉/表头 | `#21262d` | `#f0ede8` | `#1e1830` | `#182430` | `#e8e2d6` |
| `--cc-border` | 边框 | `#30363d` | `#c9c4bc` | `#2e2842` | `#243444` | `#cfc6b8` |
| `--cc-text` | 主文字 | `#e6edf3` | `#1a1814` | `#ece8f5` | `#e2edf4` | `#2a261f` |
| `--cc-text-muted` | 次要文字 | `#8b949e` | `#5c574f` | `#9b92b0` | `#7d96a8` | `#6f675c` |
| `--cc-accent` | 主强调 | `#f78166` | `#c44d2f` | `#c4a1ff` | `#3db8c9` | `#b85c28` |
| `--cc-accent-bright` | 导航 active 文字 | `#ff967d` | `#d65a3a` | `#d4b8ff` | `#5ecfe0` | `#cc6e38` |
| `--cc-success` | 成功/命中 | `#3fb950` | `#1a7f37` | `#3fb950` | `#46c880` | `#2d7a48` |
| `--cc-info` | 信息/L2 | `#58a6ff` | `#0969da` | `#79b8ff` | `#58a6ff` | `#1d6b9a` |
| `--cc-warning` | 警告 | `#d2991d` | `#9a6700` | `#d2991d` | `#c9a227` | `#8a6d1a` |
| `--cc-error` | 错误/吊销 | `#f85149` | `#cf222e` | `#f85149` | `#e85d5d` | `#b83832` |
| `--cc-tier-l0` | L0 内存缓存 | `#f78166` | `#c44d2f` | `#c4a1ff` | `#3db8c9` | `#b85c28` |
| `--cc-tier-l1` | L1 Redis | `#58a6ff` | `#0969da` | `#79b8ff` | `#58a6ff` | `#1d6b9a` |
| `--cc-tier-l2` | L2 语义缓存 | `#d2991d` | `#9a6700` | `#d2991d` | `#c9a227` | `#8a6d1a` |
| `--cc-tier-l3` | L3 上游前缀 | `#bc8cff` | `#8250df` | `#bc8cff` | `#8b9cf6` | `#7a5cad` |
| `--cc-tier-miss` | 缓存未命中 | `#484f58` | `#a8a29e` | `#5a516e` | `#5a6f80` | `#a89e90` |
| `--sidebar-width` | 侧栏宽度 | `230px` | 同左 | 同左 | 同左 | 同左 |
| `--radius-sm` / `--radius-md` | 圆角 | `6px` / `10px` | 同左 | 同左 | 同左 | 同左 |

主题切换：顶栏 `ThemeSwitcher`，`localStorage` 键 `theme`（`dark` | `light` | `midnight` | `ocean` | `sand` | `system`）。`system` 跟随 `prefers-color-scheme`，解析为 `Dark` 或 `Light`。

Dashboard 兼容别名（过渡期）：`--bg-primary`、`--accent-primary` 等映射至 `--cc-*`，见 `design-tokens.css`。

## 4. 排版

| 级别 | 大小 | 字重 | 字体 |
|------|------|------|------|
| Page title | 1.4rem (22px) | 600 | sans |
| Panel title | 0.92rem | 600 | sans |
| Body | 0.8125–0.875rem | 400–500 | sans |
| Label / 表头 | 0.68–0.8rem | 500–600 | sans, uppercase 可选 |
| Metric value | 1.75–1.9rem | 700 | mono, tabular-nums |
| Code / Token | 0.82rem | 400 | mono |

## 5. 间距

- 基准：4px
- 侧栏内边距：header `20px 18px`，nav item `8px 12px`
- 主内容：`page-content` → `28px 32px`（对齐 demo `main`）
- 卡片间距：grid `gap: 16px`
- 最大内容宽度：`1400px` 居中

## 6. 组件

### AppShell

- 结构：`aside.sidebar` + `main.main-content`
- 移动端：`MobileTopBar` + 抽屉侧栏

### Sidebar

- 类：`.sidebar-header`、`.nav-group-label`、`.nav-item`、`.nav-item.active`
- Active：`background: var(--cc-nav-active-bg)`，`color: var(--cc-accent-bright)`

### PageHeader

- 类：`.page-header`、`.page-header-title`、`.page-header-desc`、`.page-header-actions`
- 对齐 demo `.topbar`

### MetricCard

- 类：`.metric-card`、`.metric-card-label`、`.metric-card-value`、`.metric-card-sub`
- 可选：`.metric-card-trend`（`.trend.up` / `.trend.down`）

### Panel（glass-card）

- 类：`.glass-card` + `.panel-header`（标题 + `.panel-header-meta`）

### Table

- 类：`.table`；`th` uppercase、muted；行 hover `--cc-bg-elevated` 混合

### Badge

- `.badge-success` | `.badge-warning` | `.badge-error` | `.badge-info` | `.badge-accent`

### Button

- `.btn-primary`：accent 底；`.btn-secondary`：card 底 + border

### ProgressBar

- 高度 6px，`--cc-bg-elevated` 轨道，fill 用 semantic 色

## 7. 页面模板

| 模板 | 结构 |
|------|------|
| Overview | PageHeader → 4 列 MetricCard → 2:1 图表+层级 → 若干 Panel+Table |
| Table page | PageHeader → 过滤条 → glass-card > table |
| Form page | PageHeader → glass-card > 表单字段 + 底部 btn 组 |
| Empty | `.empty-state` 居中 |
| Auth | 居中 `.glass-card`，品牌区 + 输入 + 主按钮 |

## 8. 反模式

- 禁止在 UI 或 demo 中写死「99% 命中率」等未从 API 拉取的数字
- L3（`prompt_cache_hit_tokens`）与网关 L0–L2（`gateway_cache_requests_total`）分开展示
- 禁止仅用金色 accent 代表 CrabCache（规范为珊瑚色）

## 9. 同步流程

1. 修改 `crates/crab-dashboard/style/design-tokens.css`
2. 更新本文件令牌表
3. 运行 Tailwind：`cd crates/crab-dashboard && npx @tailwindcss/cli -i style/input.css -o style/output.css`
4. `bash scripts/build_dashboard.sh`
5. 确认 `docs/demo.html` 在 Dark 下与 Dashboard 一致
6. PR 勾选：已对照 DESIGN.md / design-tokens.css

GitHub Pages：`deploy-docs.yml` 将 `crates/crab-dashboard/style/design-tokens.css` 与 `docs/assets/demo-layout.css` 复制到 `site/assets/`，demo 通过 `/CrabCache/assets/` 引用。

## 10. PR 检查清单

- [ ] 仅改 tokens 即可影响 5+1 主题（含 System），无散落 hex
- [ ] 各主题键名与 Dark 一致（含 tier-l0..l3, tier-miss）
- [ ] 未引入虚构业务指标
- [ ] `cargo clippy -p crab-dashboard`（若改 Rust）通过
- [ ] `build_dashboard.sh` 通过
- [ ] table density 切换生效（comfortable / compact）
- [ ] 骨架屏替代页面级 Spinner
- [ ] 品牌色 `brand-text`（非渐变）
