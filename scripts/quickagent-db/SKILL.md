---
name: db
description: 在工作区内浏览数据库、运行只读 SQL，并与 Excel 交换数据。
---

# 数据库工作台

先用 db__list_connections 查看当前工作区关联的连接和检测到的 SQLite 文件，再用 db__inspect_schema 核对表、主键和列类型。二选一传 connection_id 或 sqlite_path。不要把凭据放进 SQL、聊天记录或工作区；连接档案由本地服务保管。

查询优先用 db__query，绑定参数（SQLite/MySQL 为 ?，PostgreSQL 为 $1、$2）。如需稳定分页，按唯一键排序并使用 next_offset。单行过大时保持 offset 不变，使用 next_fragment_offset 逐段拼接 fragment；得到完整字符串后再解析 JSON。查询计划默认不执行查询，生产环境不启用 ANALYZE。

任何写操作先展示目标环境和 SQL 供用户确认；生产环境只有明确同意才传 confirm_prod: true。只读连接不接受写入。导出走 db__export_xlsx，使用工作区相对路径，再用 Excel 插件查看。导入先使用 db__import_xlsx 的 preview 核对列和类型，确认后才能建表导入或追加。

当用户希望在左侧数据库工作台中“打开某表”、“定位/高亮某列或某行”或“滚到某条记录”时，先用 db__query 查出所需表/数据，再调用 db__focus_view({ table?, column?, row?, range?, find_column?, find_value? }) 自动切换标签页、选中对应整列/整行/单元格并滚动居中到视野内。
