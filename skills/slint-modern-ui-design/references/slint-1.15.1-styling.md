---
title: Slint 1.15.1 UI 落地速查（Style / Palette / StyleMetrics）
scope: 面向可维护、可扩展的 UI 代码组织与样式策略
---

# Slint 1.15.1 UI 落地速查（Style / Palette / StyleMetrics）

## 1) 先选内建 Style：把“系统感”交给 Slint

Slint 提供多种内建 widgets 风格（例如 Fluent / Material / Cupertino / Cosmic 等），并支持 light/dark 变体。默认 style 通常是 `native`（会根据平台映射到某个风格）。  
建议：**桌面应用优先 Fluent/Cupertino**（按平台），嵌入式/跨平台可优先 Material 或 Cosmic，再用 tokens 做品牌化差异。

选择方式（关键点）：
- 通过 `SLINT_STYLE` 环境变量在编译期选择 style
- 或在使用 build API（`slint_build`）/解释器（`slint_interpreter`）时设置 style

参考：https://docs.slint.dev/latest/docs/slint/reference/std-widgets/style/

## 2) 用 Palette/StyleMetrics 做“对齐系统”的自定义组件

当你写自定义组件（卡片、容器、分组、分隔线）时：
- 颜色优先从 `Palette` 取（例如 `Palette.background`, `Palette.border`）
- 间距优先从 `StyleMetrics` 取（`layout-padding`, `layout-spacing`）

这样做的好处：
- 你切换 fluent/material 时，自定义组件不会“跑偏”
- dark/light 自动适配更可靠

StyleMetrics 文档：https://docs.slint.dev/latest/docs/slint/reference/std-widgets/globals/stylemetrics/

## 3) 可维护的 UI 组织结构（推荐）

中型项目推荐：
- `ui/tokens.slint`：设计 tokens（颜色/排版/间距/圆角/动效）
- `ui/components/*.slint`：组件库（Button/Card/TextField/AppShell…）
- `ui/pages/*.slint`：页面组合（只做编排，不写复杂逻辑）
- `ui/app-window.slint`：Window 根组件（路由/页面切换）

规则：
- tokens 必须是单一事实来源（禁止组件内写魔法数）
- 组件文件 >200 行必须拆分
- 页面只做“拼装”，不要把业务流程写进 `.slint`

## 4) 预览与调参：用 slint-viewer 快速跑风格对比

当你需要对比 fluent/material 或 light/dark：
- 用 `slint-viewer --style <name> path/to/ui.slint`
- 或设置 `SLINT_STYLE=fluent-dark` 再预览/运行

参考：https://docs.slint.dev/latest/docs/slint/reference/std-widgets/style/

## 5) 不想从零写组件？用官方/社区组件集做底盘

Slint 提供“第三方库”列表，其中包含基于 Material Design 3 的 Material component set，以及其他组件库。  
策略建议：
- 想要“稳 + 快”：选组件集做底盘 + 少量品牌化 tokens
- 想要“独特但不怪”：在底盘基础上只改：排版、留白、角半径、边框、微阴影与交互状态

参考：https://docs.slint.dev/latest/docs/slint/guide/development/third-party-libraries/

