---
title: 现代化 UI 设计指南（可执行版）
scope: 适用于桌面/嵌入式/移动的“简洁现代”UI；偏向可维护的设计系统落地。
---

# 现代化 UI 设计指南（可执行版）

目标：做出“像认真设计过”的 UI，而不是“AI 拼贴出来的 UI”。核心方法是：**系统化**（tokens/组件/规则）> **特效化**（渐变/发光/重阴影）。

## 1) 信息层级（Hierarchy）

你必须能在 3 秒内回答：
- 这个页面的主任务是什么？
- 主信息在哪里？次级信息在哪里？
- 主要 CTA 是哪个？次要操作在哪里？

落地做法：
- 每个页面只允许 1 个主标题（视觉最强）
- 每个“模块”必须有清晰的标题/边界（标题、分组、留白或轻边框）
- 重要信息用层级表达，不用同时叠加：更大 + 更粗 + 更亮 + 更彩（会显廉价）

## 2) 间距与节奏（Spacing Rhythm）

建议用 4/8 的间距体系（可根据平台密度调）：
- 基础单位：4px
- 常用级：8/12/16/24/32/40（选择少量固定档位）

检查点：
- 同类组件之间的间距必须一致（列表行、表单项、卡片之间）
- “组件内 padding”和“组件间 margin”不要混用同一数值档（例如：内 12px 外 24px）

## 3) 排版（Typography）

规则：
- 用**少而稳定**的字号/字重档位（例如 6 档）
- 小字要更高对比度/更合适行距；不要靠“更细更灰”营造高级感

实践建议：
- Title 用较高字重（600/700），Body 400/450，Caption 400
- 对齐：多列信息保持同一 baseline/左对齐线

## 4) 颜色（Color）与对比度（Contrast）

原则：
- 背景/表面层级用中性；强调色只用于强调（不要全屏紫蓝发光）
- 语义色（success/warn/error）用于状态，不用于装饰

可访问性最低线（WCAG 常用阈值）：
- 小文本对比度 ≥ **4.5:1**
- 大文本（14pt bold / 18pt regular 以上）与图形对比度 ≥ **3:1**
- Disabled 状态不强制满足对比度，但要避免“看不见”导致误操作

## 5) 组件状态（States）必须齐全

每个可交互组件至少要定义：
- default / hover / pressed / focus / disabled
- 表单还要：invalid（错误态）与 helper（说明态）

常见差评点：
- 只有 hover，没有 focus → 键盘用户不可用
- disabled 与 default 太像 → 用户不确定是否可点
- 错误信息模糊 → 用户不知道怎么修

## 6) 动效（Motion）克制但必须存在

目的：提供反馈与连续性，不是炫技。

建议：
- 常用动效时长：120–180ms（快速反馈）、220–320ms（页面级过渡）
- easing 用轻微缓出（不要弹簧乱跳）
- 只给“状态变化”加动效：hover、展开/收起、加载切换

## 7) 反 AI 劣质 UI：10 条快速自检

命中任意 3 条，UI 就会“很 AI”：
1) 到处都是渐变 + 发光
2) 同一个按钮在不同页面圆角不一致
3) 组件间距没有规律（10/13/17/19px 混用）
4) 文字层级只有“更大更粗”一种手段
5) 空态只有“暂无数据”四个字，没有下一步动作
6) 错误提示只写“失败”，不写原因与解决方案
7) 强调色使用面积过大（>10%）
8) 阴影过重导致脏与浮夸
9) 可点区域太小（尤其图标按钮）
10) 没有 focus 状态、没有键盘可达路径

## 参考链接（用于原则对齐）

- Slint 内建 Widget Styles（Fluent/Material 等）与 Palette/StyleMetrics 用法：https://docs.slint.dev/latest/docs/slint/reference/std-widgets/style/  
- StyleMetrics（layout-spacing/layout-padding）：https://docs.slint.dev/latest/docs/slint/reference/std-widgets/globals/stylemetrics/  
- Material 3 对比度建议（3:1 / 4.5:1）：https://m3.material.io/foundations/designing/color-contrast  
- Windows/Fluent 设计指南入口（布局、排版、动效等）：https://learn.microsoft.com/windows/apps/design/guidelines-overview  

