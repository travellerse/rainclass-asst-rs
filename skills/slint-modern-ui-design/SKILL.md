---
name: slint-modern-ui-design
description: Use when 用户要为 Slint（1.15.1）应用设计/重构界面（信息架构、布局、排版、颜色、组件库、主题、交互状态、动效与可用性），并希望做出简洁现代、用户友好、视觉精致且与“AI 劣质 UI”明显不同的可维护 UI 代码。
---

# Slint 现代化 UI 设计 Skill（1.15.1）

## Goal

把“现代、简洁、精致、可维护”的 UI 设计原则落地到 **Slint 1.15.1** 的可运行 `.slint` 代码中：建立可扩展的设计系统（tokens/组件/布局规则），并输出具有明确视觉层级、优秀可用性与一致交互状态的界面。

## Version Target（适配版本）

- 目标：Slint **v1.15.1**
- 允许：1.15.x（原则与结构可复用）
- 关键依赖能力：内建 widget styles（fluent/material/cosmic/cupertino 等）、`Palette`、`StyleMetrics`、可通过 `SLINT_STYLE` 选择样式。

## When to Use (Triggers)

当用户提出以下类型请求时触发：
- “帮我用 Slint 设计一个现代化 UI / 让 UI 更高级”
- “我的 Slint 界面太像 AI 生成的了，想变得更有品味”
- “做一套 Slint 组件库/设计系统（主题、按钮、卡片、输入框、导航）”
- “Slint 的 fluent/material 风格怎么选？怎么做自定义但不崩？”
- “把这个 demo 的 UI 重新设计，要求简洁、用户友好、可扩展”

## Intake（最小信息）

只问会显著影响结果的点；用户没给就按默认假设继续，并在输出里写清楚假设。

1) 平台与风格基线
- 目标平台：Windows / macOS / Linux / Android / 嵌入式
- 风格偏好：Fluent / Material / 自定义品牌（或“给我你推荐的”）

2) 信息架构与密度
- 页面/模块：例如 Dashboard / 列表详情 / 设置 / 登录
- 信息密度：紧凑 / 标准 / 宽松

3) 品牌与视觉倾向
- 主色/品牌色（或“中性高级”）
- 明/暗主题：仅明 / 仅暗 / 明暗都要

4) 可用性约束
- 字体与语言：是否中英混排/国际化
- 无障碍：是否必须满足对比度、键盘可达、明确焦点态

## Workflow（执行步骤）

### 0) 先选“风格底盘”，再做差异化

默认策略（为了避免“劣质自绘控件”）：
1) **优先选择内建 style**（fluent/material 等）作为底盘
2) **用 tokens + 少量组件封装**拉开差异（而不是全套重写）
3) 使用 `Palette`/`StyleMetrics` 与 style 同步，保证“像同一个系统”

### 1) 定义设计系统：Tokens（颜色/排版/间距/圆角/阴影/动效）

输出一个 `ui/tokens.slint`（或 `theme.slint`）作为单一事实来源：
- 间距：基于 4/8px 的 scale（并与 `StyleMetrics.layout-spacing/layout-padding` 对齐）
- 排版：最少 6 个层级（display/headline/title/body/label/caption）
- 颜色：分为**语义色**（success/warn/error）与**表面层级**（bg/surface/card）
- 交互状态：hover/pressed/focus/disabled 的颜色与透明度
- 阴影：少量档位（sm/md/lg），避免“重阴影 + 玻璃拟态滥用”

要求：tokens 命名必须以“语义”命名（`surface-1`、`text-muted`、`border-subtle`），禁止以“感觉”命名（`nice-purple`）。

### 2) 建立布局规则：网格、对齐、留白与阅读节奏

把布局当成系统，而不是堆控件：
- 全局 padding 与 page gutter 统一（从 tokens/StyleMetrics 读取）
- 主内容最大宽度/列宽可控（避免一屏内容太散或太拥挤）
- 使用一致的对齐线（Left edges 对齐；按钮/输入框高度统一）

Slint 落地方式：
- 使用 `VerticalLayout/HorizontalLayout/GridLayout` 等布局组件
- 需要网格化内容时，优先 `GridLayout`（配合 `for/if` 做数据驱动）
- 长内容滚动：用 `Flickable/ScrollView` 并保证顶部标题区保持稳定

### 3) 组件库最小闭环（先做 6 个“核心组件”）

先完成可复用的核心组件，再铺页面：
1) AppShell（标题区/侧边栏/主内容容器）
2) Button（Primary/Secondary/Ghost，含 loading/disabled）
3) TextField（label/help/error，含 focus/invalid）
4) Card（层级、边框、hover、可点击）
5) ListRow（密度、分隔、选中态、空态）
6) Toast/Dialog（错误提示与关键确认）

原则：
- 每个组件都必须包含：默认/hover/pressed/focus/disabled（至少 4 个状态）
- 所有颜色/间距/圆角/动效都只能从 tokens 读取
- 超过 ~200 行的 `.slint` 文件必须拆分为子组件

### 4) “反 AI 劣质 UI”差异化策略（强制执行）

你必须显式做出这些差异化（至少命中 5 条）：
- 明确的信息层级：标题/副标题/正文/注释有可见差异（不靠“更大更粗”一种手段）
- 规律的间距节奏：同类模块间距一致，组件内部 padding 有系统（不是随手 10/13/17px）
- 颜色克制：背景/卡片不要大面积渐变；强调色只用于强调（<10% 面积）
- 可读性优先：小字对比度至少 4.5:1；大字/图形至少 3:1（除 disabled）
- 状态完整：空态/加载态/错误态/不可用态都有明确文案与动作
- 交互可预测：点击区足够大、hover 反馈一致、焦点态明确
- 微文案：按钮/错误提示避免“确定/取消”泛滥，写清动作含义
- 视觉细节：边框/阴影/圆角统一且轻；避免“厚重阴影 + 过度玻璃”

### 5) 评审与迭代（交付前必做 2 轮）

每轮输出一个“自评清单”并修正：
1) 结构轮：布局与信息架构是否清晰？空态/错误态是否齐全？
2) 视觉轮：间距节奏是否统一？排版层级是否合理？对比度是否达标？

Slint 工具化建议：
- 用 `slint-viewer` 或 IDE 预览快速迭代不同 style（`SLINT_STYLE=fluent-dark` 等）

## Resource Use（本 skill 自带参考与模板）

如 skill 包中存在以下文件：
- `references/modern-ui-guidelines.md`：现代 UI 设计原则 + 可执行检查清单
- `references/slint-1.15.1-styling.md`：Slint 1.15.1 的 style/Palette/StyleMetrics 用法与落地建议
- `assets/slint/`：可复制的 tokens 与组件模板（用于快速起步，不是强制）

执行时优先阅读 references，再决定是否拷贝 assets 作为项目起点。

## Output Contract（你必须产出什么）

当用户要“设计/重构 UI”时，至少交付：
1) 设计系统：tokens 文件（颜色/排版/间距/圆角/动效）
2) 组件库：至少 6 个核心组件（见 Workflow 第 3 步）
3) 页面：至少 1 个完整页面（含空/载/错/禁用状态）
4) 使用说明：目录结构、命名约定、扩展方式（如何新增组件/页面）

## Quality Bar（硬性质量标准）

必须满足：
- 视觉：有明确层级、留白节奏统一、强调色克制、细节一致
- 可用性：可点击区域合理、状态完整、焦点态清晰、对比度达标（除 disabled）
- 可维护：tokens 单一来源；组件不硬编码魔法数；文件可拆分、命名一致
- 适配：优先基于 Slint 内建 style；自定义不应破坏系统一致性

禁止项（出现任意 1 条视为不合格）：
- 页面靠堆叠渐变/发光/厚阴影“装高级”
- 同一类元素出现多套间距/圆角/阴影标准
- 没有空态/错误态/不可用态
- 关键文本可读性差（对比度不足、字号过小、行距拥挤）

