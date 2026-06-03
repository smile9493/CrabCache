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
- **标题样式**：使用 `Outfit` 字体，品牌强调色 `--cc-accent`（类名 `.brand-text`）
- **产品名**：CrabCache

## 3. 设计令牌

语义前缀 `--cc-*`。五套主题（`theme-dark` / `theme-light` / `theme-midnight` / `theme-ocean` / `theme-sand`）必须包含**相同键名**。

| Token | 用途 | Dark | Light | Midnight | Ocean | Sand |
|-------|------|------|-------|----------|-------|------|
| `--cc-bg` | 主内容区背景 | `oklch(0.12 0.008 30)` | `oklch(0.96 0.005 35)` | `oklch(0.10 0.012 280)` | `oklch(0.11 0.010 220)` | `oklch(0.94 0.008 70)` |
| `--cc-bg-sidebar` | 侧栏/顶栏背景 | `oklch(0.09 0.006 30)` | `oklch(0.92 0.008 35)` | `oklch(0.07 0.008 280)` | `oklch(0.08 0.008 220)` | `oklch(0.89 0.012 70)` |
| `--cc-bg-card` | 卡片/面板 | `oklch(0.16 0.010 30)` | `oklch(1.00 0.002 35)` | `oklch(0.14 0.015 280)` | `oklch(0.15 0.012 220)` | `oklch(0.97 0.004 70)` |
| `--cc-bg-elevated` | 悬停/下拉/表头 | `oklch(0.22 0.012 30)` | `oklch(0.90 0.010 35)` | `oklch(0.18 0.020 280)` | `oklch(0.20 0.015 220)` | `oklch(0.87 0.015 70)` |
| `--cc-border` | 边框 | `oklch(0.26 0.014 30)` | `oklch(0.80 0.015 35)` | `oklch(0.24 0.025 280)` | `oklch(0.24 0.018 220)` | `oklch(0.78 0.015 70)` |
| `--cc-text` | 主文字 | `oklch(0.92 0.010 30)` | `oklch(0.18 0.015 35)` | `oklch(0.92 0.015 280)` | `oklch(0.92 0.010 220)` | `oklch(0.22 0.015 70)` |
| `--cc-text-muted` | 次要文字 | `oklch(0.68 0.012 30)` | `oklch(0.48 0.018 35)` | `oklch(0.68 0.020 280)` | `oklch(0.68 0.015 220)` | `oklch(0.50 0.018 70)` |
| `--cc-accent` | 主强调 | `oklch(0.65 0.16 28)` | `oklch(0.50 0.15 28)` | `oklch(0.78 0.12 280)` | `oklch(0.70 0.12 200)` | `oklch(0.48 0.12 45)` |
| `--cc-accent-bright` | 导航 active 文字 | `oklch(0.72 0.18 28)` | `oklch(0.45 0.17 28)` | `oklch(0.84 0.14 280)` | `oklch(0.76 0.14 200)` | `oklch(0.42 0.14 45)` |
| `--cc-success` | 成功/命中 | `oklch(0.72 0.15 142)` | `oklch(0.52 0.13 142)` | `oklch(0.72 0.15 142)` | `oklch(0.72 0.15 142)` | `oklch(0.48 0.12 142)` |
| `--cc-info` | 信息/L2 | `oklch(0.70 0.14 250)` | `oklch(0.48 0.15 250)` | `oklch(0.75 0.12 250)` | `oklch(0.72 0.12 220)` | `oklch(0.48 0.12 250)` |
| `--cc-warning` | 警告 | `oklch(0.78 0.15 80)` | `oklch(0.58 0.14 80)` | `oklch(0.78 0.15 80)` | `oklch(0.78 0.15 80)` | `oklch(0.52 0.12 80)` |
| `--cc-error` | 错误/吊销 | `oklch(0.62 0.18 22)` | `oklch(0.46 0.17 22)` | `oklch(0.65 0.16 22)` | `oklch(0.65 0.16 22)` | `oklch(0.45 0.15 22)` |
| `--cc-tier-l0` | L0 内存缓存 | `oklch(0.65 0.16 28)` | `oklch(0.50 0.15 28)` | `oklch(0.78 0.12 280)` | `oklch(0.70 0.12 200)` | `oklch(0.48 0.12 45)` |
| `--cc-tier-l1` | L1 Redis | `oklch(0.70 0.14 250)` | `oklch(0.48 0.15 250)` | `oklch(0.75 0.12 250)` | `oklch(0.72 0.12 220)` | `oklch(0.48 0.12 250)` |
| `--cc-tier-l2` | L2 语义缓存 | `oklch(0.78 0.15 80)` | `oklch(0.58 0.14 80)` | `oklch(0.78 0.15 80)` | `oklch(0.78 0.15 80)` | `oklch(0.52 0.12 80)` |
| `--cc-tier-l3` | L3 上游前缀 | `oklch(0.70 0.16 300)` | `oklch(0.48 0.16 300)` | `oklch(0.78 0.12 280)` | `oklch(0.70 0.14 300)` | `oklch(0.46 0.13 300)` |
| `--cc-tier-miss` | 缓存未命中 | `oklch(0.40 0.010 30)` | `oklch(0.65 0.010 35)` | `oklch(0.42 0.015 280)` | `oklch(0.42 0.012 220)` | `oklch(0.68 0.010 70)` |
| `--sidebar-width` | 侧栏宽度 | `230px` | 同左 | 同左 | 同左 | 同左 |
| `--radius-sm` / `--radius-md` | 圆角 | `6px` / `10px` | 同左 | 同左 | 同左 | 同左 |

主题切换：顶栏 `ThemeSwitcher`，`localStorage` 键 `theme`（`dark` | `light` | `midnight` | `ocean` | `sand` | `system`）。`system` 跟随 `prefers-color-scheme`，解析为 `Dark` 或 `Light`。

Dashboard 兼容别名（过渡期）：`--bg-primary`、`--accent-primary` 等映射至 `--cc-*`，见 `design-tokens.css`。

## 4. 排版

| 级别 | 大小 | 字重 | 字体 |
|------|------|------|------|
| Page title | 1.4rem (22px) | 600 | display (Outfit) |
| Panel title | 0.95rem | 700 | display (Outfit) |
| Body | 0.8125–0.875rem | 400–500 | sans (Inter) |
| Label / 表头 | 0.68–0.8rem | 500–600 | sans (Inter), uppercase 可选 |
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
