---
name: db
description: 在工作区内浏览数据库、运行只读 SQL，并与 Excel 交换数据。
---

# 数据库工作台

先用 db__list_connections 查看当前工作区关联的连接和检测到的 SQLite 文件，再用 db__inspect_schema 核对表、主键和列类型。二选一传 connection_id 或 sqlite_path。不要把凭据放进 SQL、聊天记录或工作区；连接档案由本地服务保管。

查询优先用 db__query，绑定参数（SQLite/MySQL 为 ?，PostgreSQL 为 $1、$2）。如需稳定分页，按唯一键排序并使用 next_offset。单行过大时保持 offset 不变，使用 next_fragment_offset 逐段拼接 fragment；得到完整字符串后再解析 JSON。查询计划默认不执行查询，生产环境不启用 ANALYZE。

任何写操作先展示目标环境和 SQL 供用户确认；生产环境只有明确同意才传 confirm_prod: true。只读连接不接受写入。导出走 db__export_xlsx，使用工作区相对路径，再用 Excel 插件查看。导入先使用 db__import_xlsx 的 preview 核对列和类型，确认后才能建表导入或追加。

## UI 操作 vs SQL 过滤（核心原则：能用 UI 定位就用 UI，保留用户上下文）
- **能直接操作 UI 定位/浏览的，优先调用 `db__focus_view`（保留整表全部列与前后相邻行）**：
  - 打开/找某张表、高亮选中整列或整行、在表中定位并滚到某条记录 → 先在需要时用 `db__inspect_schema` 确认表名/列名，然后直接调用 `db__focus_view({ connection_id?, schema?, table?, column?, row?, offset?, range?, find_column?, find_value? })`。
  - 这会像人工一样在左侧树展开高亮该表、打开完整表视图并滚到目标位置高亮，用户可以顺便继续查看相邻的列与行；大表目标不在首页时，可传 `offset` 跳到目标所在分页再配合 `find_column`/`find_value` 滚动高亮，而无需用 `WHERE` 过滤掉其余数据。
  - **禁止**为了“高亮某列”或“定位某条记录”去跑 `SELECT 单列` 或 `LIMIT 1` SQL 把整表缩成单列/单行。
- **用户明确要求「只看某某数据」「按条件过滤」「聚合统计 / 联查 / 排序」或「数据写入」→ 用 SQL（`db__query` / `db__execute`）**：
  - `db__query` 默认静默在后台执行（`show_in_ui` 默认为 `false`），做分析/计数/抽样探针时**不会**抢占或覆盖用户当前打开的表标签。
  - 只有当用户明确要求**在左侧表格界面展示**某条过滤/排序/联查结果（如“只看张三今天的日报，帮我列在表里”、“过滤金额 > 1000 的订单给我看”）时，才在 `db__query` 中传 `show_in_ui: true`（同步到独立「AI 查询」标签，不破坏整表标签）；若还需在结果里高亮某列/某行，可在之后调一次不传 `table` 的 `db__focus_view({ column?, row? })`。
