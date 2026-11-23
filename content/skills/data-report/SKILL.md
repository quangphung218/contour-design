---
name: data-report
zh_name: "数据可视化报告"
en_name: "Data Visualization Report"
emoji: "📊"
description: "Turns CSV, Excel, or JSON data into a polished visual report page."
zh_description: "把 CSV/Excel/JSON 数据转成漂亮的可视化报告页"
en_description: "Turns CSV, Excel, or JSON data into a polished visual report page."
category: data
scenario: finance
aspect_hint: "桌面长页面"
featured: 10
tags: ["data", "report", "chart", "数据", "报告"]
example_id: sample-data-weekly-report
example_name: "数据报告 · 周报"
example_format: csv
example_tagline: "KPI 卡 + SVG 图表 + 表格"
example_desc: "9 个月增长数据自动渲染成可视化报告, 内联 SVG 图表"
triggers:
  - "dashboard"
  - "data report"
  - "kpi report"
  - "infographic"
od:
  mode: prototype
  surface: web
  platform: desktop
  scenario: finance
  upstream: "https://github.com/nexu-io/html-anything"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  example_prompt: "Use the Data Visualization Report template to turn my CSV, Excel, or JSON data into a polished visual report page. Preserve the template's visual signature, use real content and data, and avoid lorem ipsum or placeholder images."
  example_prompt_i18n:
    zh-CN: "用「数据可视化报告」模板把我的内容做成一份「把 CSV/Excel/JSON 数据转成漂亮的可视化报告页」。保持模板的视觉签名，使用真实内容和数据，避免 lorem ipsum 和占位图片。"
---

【模板: 数据可视化报告】
- 头部: 报告标题 + 时间区间 + 数据来源说明。
- KPI 卡片网格: 3-5 个最重要指标, 每个卡片显示数值 + 同比变化 + 微型趋势线。
- 主图表区: 至少 2 个图表 (柱状 / 折线 / 饼 / 散点), 用内联 SVG 手绘 (参考 example.html 的 `chart()` / `spark()`), 不引图表库, 也不从 CDN 引入任何文件 (预览会拦截外部请求), 数据从用户输入解析得到。
- **图表容器必须有固定高度**: 每个图表外层包一个 `<div style="position:relative;height:NNNpx">` (KPI 迷你图 ~40px, 主图表 ~240–280px), SVG 画在其中 `position:absolute;inset:0` 的元素里, 按它的 `clientWidth` / `clientHeight` 绘制, 窗口 resize 时重画。
- 数据表格: 用户原始数据节选, 使用 `<table>` + 现代化样式 (zebra stripe, hover, sticky header)。
- 洞察块: 3-5 条文字洞察, 用 emoji 开头, 像产品周报。
- 底部"方法论"折叠区。
- 配色克制专业: 主色 1 + 中性色阶, 图表用调色板。
- **必须解析用户提供的实际数据**, 不要捏造。
