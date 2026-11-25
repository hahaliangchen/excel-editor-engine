---
name: excel
description: 使用本地 Excel 工作表与公式工具处理 .xlsx 文件。
---

# Excel 表格

只处理工作区中的 .xlsx。工具的 path 必须是工作区相对路径，目录需已存在；不要传入绝对路径、.. 或符号链接。

- excel__inspect：检查工作表尺寸和前 3 行。excel__read_range：按 A1 区域读 raw、formula、value、display 与样式；公式不含起始等号。
- excel__write_cells：传 A1 地址到字符串、数字、布尔值或 null 的映射。以等号起始表示公式，单引号前缀保留文本类型；一次调用原子写入并重算。
- excel__manage_rows_cols：change.action 可为 insert_rows、delete_rows、insert_columns、delete_columns、set_width、freeze_first_row；行列均从 1 开始。
- excel__manage_sheets：change.action 可为 new、rename、copy、delete；保留至少一张工作表。
- excel__format_range：支持 bold、italic、color、fill、align、numberFormat。merge 为 true 时仅在不会遮蔽非空单元格的情况下合并；false 取消合并。
- excel__search：按显示值或公式查找；excel__create_xlsx：传 workbook.sheets，每张表含 name 和以 A1 为键的 cells（每项含 raw，可带 style）以及可选 merges / widths / frozen。默认不覆盖。
- excel__focus_view：在左侧 Excel 工作台中切换工作表、高亮选中指定列（支持列字母如 `F` 或首行表头名如 `日报内容`）、行、单元格或 A1 区域，并自动横纵滚动到视野内。

分页工具返回 next_index。超长项以 fragment.text 分段返回；用同一个 start_index 与 next_fragment_offset 续读，按顺序拼接文本后解析 JSON。遇到 next_index 为 null 表示结束。批量写入的回包为摘要；后续用 excel__read_range 精确查证计算值。

建议先 inspect，读取最小范围再写入；修改后核对受影响的汇总工作表。不确定是否覆盖时保留原文件，另存到新路径。
