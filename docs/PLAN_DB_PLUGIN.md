# 数据库工作台（DB Studio）插件施工计划（`excel-editor-engine` 单仓双插件）

> **适用仓库**：`/home/goodtime/project/excel-editor-engine`
> **交付产物**：安装到 `/home/goodtime/Documents/project/codex-workspace-mcp/bundled_plugins/db` 的独立 QuickAgent 插件包（与现有 `bundled_plugins/excel` 完全解耦、共存，**严禁破坏现有 `excel` 插件的功能、测试与打包脚本**）。
>
> **角色分工**：本计划作为 QuickAgent 开发施工规格书；完成后交由 Antigravity 逐项执行代码审查、Rust 单测、前端构建、`verify.mjs` 打包自检与 Playwright 端到端验收。

---

## 0. 全局目标与红线

1. **单仓双插件，严禁回归 `excel`**：
   - 保持 `packages/excel-studio`、`rust-excel`（`excel-server`）和 `scripts/bundle-quickagent.sh` 行为完全兼容，所有现有 `npm test`、`cargo test`、`scripts/quickagent/verify.mjs` 100% 绿灯。
   - 新增独立数据库工作台与后端服务，输出独立插件目录 `bundled_plugins/db`（`id: "db"`, `library: "db"`）。
2. **最大化复用 Excel 能力，形成王牌联动**：
   - 复用/抽离 `excel-studio` 的高性能表格样式与网格交互能力（单元格选中、列宽拖拽、双主题 Paper/Dark Ink、行内编辑、复制粘贴），用于展示 SQL 结果集与表数据；
   - 后端直接复用 `rust-excel` 的 `.xlsx` 编解码能力（`codec` / `model` / `umya-spreadsheet`），实现 **SQL 查询结果一键导出工作区 `.xlsx`** 以及 **工作区 `.xlsx` 一键导入建表/灌入数据**。
3. **生产级安全边界（核心红线）**：
   - **连接凭据隔离**：连接配置与密码存于用户私有目录（如 `~/.config/grow-db/connections.json` 或环境变量 `DB_STUDIO_DATA_DIR` 指定目录，文件权限严格 `0600`），**绝不把密码写进目标项目仓库**，任何 API 响应与 AI 工具输出中密码字段必须完全省略或脱敏为 `"[redacted]"`。
   - **环境分级保护（`dev` / `staging` / `prod`）**：
     - `db__query` **强制只读**：仅允许 `SELECT` / `WITH ... SELECT` / `SHOW` / `DESCRIBE` / `EXPLAIN` / `PRAGMA`（且禁止多语句分号注入），默认自动限制最大返回行数（`limit` 默认 `200`，上限 `1000`），防大表撑爆 AI 上下文；
     - `db__execute`（`INSERT` / `UPDATE` / `DELETE` / DDL）在 `env == "prod"` 时，默认拒绝执行，必须显式传 `confirm_prod: true` 且 `read_only` 连接永不允许写操作；
     - 所有涉及工作区本地文件（SQLite 文件打开、`.xlsx` 导出/导入）必须使用 `cap-std` / canonicalize 严格限制在 `workspace_root` 内，拒绝路径穿越与符号链接越界。
4. **严格遵守 QuickAgent `plugin.json` 契约**：
   - QuickAgent 的 `CommandTool` 反序列化启用了 `#[serde(deny_unknown_fields)]`，`command_tools` 数组中的每个元素**只允许**包含：`name`、`description`、`input_schema`、`output_schema`（可选）、`program`、`args`、`timeout_seconds`（1..=600），禁止添加额外未知字段。
   - 所有 `--tool` RPC 响应单次严格控制在 `200 KB` 以内（与 `rust-excel/src/tool_rpc.rs` 的 `MAX_OUT = 200 * 1024` 及分页协议保持一致）。

---

## 1. 仓库目录规划（Monorepo 布局）

在现有 `excel-editor-engine` 中平滑扩展，不打散原结构：

```text
excel-editor-engine/
├── package.json                        # workspaces 扩展为 ["packages/excel-studio", "packages/db-studio"]（可选抽 packages/grid-shared）
├── packages/
│   ├── excel-studio/                   # 现有 Excel Studio（保持零回归；可抽离共享 grid/style 模块）
│   └── db-studio/                      # 新增：DB Studio 前端（Vite 打包为单页独立资产，零外部 CDN）
│       ├── index.html
│       ├── package.json
│       ├── vite.config.ts
│       ├── tsconfig.json
│       └── src/
│           ├── main.ts
│           ├── api-client.ts           # 调用同源 /api/*（兼容 QuickAgent local-api 反向代理路径）
│           ├── store.ts                # 连接列表、Schema 树、多 Tab SQL 编辑器、结果集与脏行草稿
│           ├── grid.ts                 # 复用 excel-studio 网格引擎，支持排序/筛选/NULL 标识/行内编辑生成 UPDATE 预览
│           ├── sql-editor.ts           # 轻量 SQL 编辑器（快捷键 Ctrl/Cmd+Enter 执行、表/列名补全提示、多标签页、历史记录）
│           ├── components/             # 连接管理弹窗、Schema 树、结果工具栏（分页/导出 .xlsx/导入 .xlsx）、EXPLAIN 视图
│           └── styles/studio.css       # 严格对齐 QuickAgent Light Paper / Dark Ink 双主题
├── rust-excel/                         # 保留为现有库 + excel-server 二进制（Cargo.toml 可升为 workspace 或增建 rust-db crate）
├── rust-db/                            # 新增：独立 Rust Crate，产出 `db-server` 二进制（依赖 `rust-excel` 库复用 XLSX 编解码）
│   ├── Cargo.toml                      # axum 0.8 + tokio + sqlx 0.8 (sqlite, mysql, postgres, runtime-tokio, tls-rustls) + cap-std + excel-editor-engine
│   ├── src/
│   │   ├── main.rs                     # `--tool` 进入 stdio JSON-RPC；无参启动 HTTP local_service（读 BIND 环境变量）
│   │   ├── config.rs                   # 连接档案存储（0600 权限）、workspace 历史绑定、密码脱敏
│   │   ├── drivers/
│   │   │   ├── mod.rs                  # 统一 DbDriver trait（test_conn, list_schemas, list_tables, describe_table, query_readonly, execute_write, explain）
│   │   │   ├── sqlite.rs               # SQLite 驱动（支持工作区相对路径 .db/.sqlite/.sqlite3 与绝对路径只读/读写）
│   │   │   ├── mysql.rs                # MySQL / MariaDB 驱动（sqlx::MySqlPool，完整类型序列化 DECIMAL/DATETIME/JSON/BLOB）
│   │   │   └── postgres.rs             # PostgreSQL 驱动（sqlx::PgPool，支持多 schema、UUID/JSONB/NUMERIC/ARRAY）
│   │   ├── sql_guard.rs                # 严格只读 SQL 分类器（去注释/字符串字面量、禁多语句、白名单前缀、自动包裹/追加 LIMIT）
│   │   ├── xlsx_bridge.rs              # 复用 rust-excel：查询结果集 → .xlsx 导出；.xlsx → 推断列类型并预览/导入建表
│   │   ├── server.rs                   # Axum HTTP 服务（/api/health, /api/connections, /api/schema, /api/query, /api/execute, /api/xlsx/*）
│   │   └── tool_rpc.rs                 # QuickAgent `--tool` stdio JSON-RPC 分发与 200KB 分页预算控制
│   └── tests/
│       └── db_engine.rs                # SQLite 真实端到端测试、只读守卫测试、prod 保护测试、XLSX 导出导入往返测试
├── scripts/
│   ├── bundle-quickagent.sh            # 保留不动（继续打包 bundled_plugins/excel）
│   ├── bundle-quickagent-db.sh         # 新增：构建 db-studio + db-server，原子安装到 bundled_plugins/db
│   ├── browser-smoke-db.mjs            # 新增：Playwright 双主题端到端验证脚本（含临时 SQLite 建库查询与导出）
│   └── quickagent-db/
│       ├── manifest.mjs                # 生成 bundled_plugins/db/plugin.json
│       ├── SKILL.md                    # AI 使用规范（先列连接/查 Schema、优先只读、写前确认、结合导出 Excel）
│       └── verify.mjs                  # 打包后自动校验：manifest、离线资源、HTTP health、stdio RPC 闭环
└── docs/
    └── PLAN_DB_PLUGIN.md               # 本计划文档
```

> **为什么拆成独立 `rust-db/` crate 产出 `db-server`？**
> 1. 保持 `excel-server` 体积和编译依赖完全干净（不把 `sqlx` 的 SQLite/MySQL/Postgres 驱动塞进 `excel-server`）；
> 2. `rust-db` 通过 path 依赖 `excel-editor-engine = { path = "../rust-excel" }` 直接复用 `.xlsx` 读写引擎，零代码复制；
> 3. `sqlx 0.8` 启用 `features = ["runtime-tokio", "tls-rustls-ring-webpki", "sqlite", "mysql", "postgres", "chrono", "rust_decimal", "json", "uuid"]`（或与当前 Cargo 版本匹配的 `tls-rustls` 特性），纯 Rust 实现，无外部 OpenSSL / libmysqlclient 系统依赖。

---

## 2. 数据与连接管理设计

### 2.1 连接配置档案（`connections.json`，权限 `0600`）
默认存储路径：`$DB_STUDIO_DATA_DIR/connections.json`（未设置时回退 `~/.config/grow-db/connections.json`）。

```json
{
  "version": 1,
  "connections": [
    {
      "id": "rag-go-dev-mysql",
      "name": "rag-go 开发库",
      "driver": "mysql",
      "env": "dev",
      "read_only": false,
      "host": "10.10.10.201",
      "port": 3306,
      "database": "rag_go",
      "username": "root",
      "password": "secret",
      "ssl_mode": "preferred",
      "file_path": null,
      "workspace_roots": ["/home/goodtime/project/rag-go"],
      "last_used_at": "2026-11-24T09:30:00Z",
      "notes": "包含 rag_kg_jobs 与文档元数据表"
    }
  ]
}
```

- **工作区自动关联（项目记忆）**：
  - 每次在某 `workspace_root` 下使用某连接查询，自动把该 `workspace_root` 记入 `workspace_roots` 并更新 `last_used_at`；
  - 调用 `db__list_connections` 时，**优先排序并返回当前 `workspace_root` 关联过的连接**，并且自动扫描当前工作区根目录下可见的轻量 `.sqlite` / `.sqlite3` / `.db` 文件作为「工作区本地 SQLite 发现项」一并提示，AI 立刻知道本项目用哪个库！
- **凭据脱敏红线**：
  - 任何 HTTP GET 或 `db__*` 工具返回中**绝不包含 `password` 明文**（只返回 `has_password: true/false`）；
  - 前端编辑已有连接时，密码框留空表示「保留原密码」，只有用户输入新密码才覆盖。

---

## 3. QuickAgent 插件契约（`bundled_plugins/db/plugin.json`）

- `id`: `"db"`
- `library`: `"db"`
- `title`: `"数据库"`
- `enabled_by_default`: `true`
- `skill`: `"SKILL.md"`
- `pages`: `[{ "id": "studio", "title": "数据库", "entry": "page/index.html", "renderer": "html" }]`
- `local_service`: `{ "program": "bin/db-server" }`
- `attachment_extensions`: `["db", "sqlite", "sqlite3"]`

### 暴露 8 个核心 AI 工具（`command_tools`，全部走 `bin/db-server --tool`）

| 工具名 | 说明 | 关键入参 |
| :--- | :--- | :--- |
| `db__list_connections` | 列出已保存连接（按当前工作区相关性排序，隐藏密码）+ 发现工作区内的本地 SQLite 文件 | `include_workspace_sqlite?: boolean` |
| `db__save_connection` | 新增或测试保存数据库连接档案（密码落 `0600` 凭据库，返回脱敏摘要与连通性测试结果） | `id?`, `name`, `driver` (`sqlite\|mysql\|postgres`), `env` (`dev\|staging\|prod`), `read_only?`, `host?`, `port?`, `database?`, `username?`, `password?`, `file_path?`, `test_connection?` |
| `db__inspect_schema` | 查看数据库列表、表清单、行数估算，或查看指定表的完整列定义、主键、索引、外键与 `CREATE TABLE` DDL | `connection_id` 或 `sqlite_path`, `schema?`, `table?`, `include_ddl?`, 分页参数 |
| `db__query` | **严格只读 SQL 查询**（校验只读、禁多语句、自动限行分页），返回列名、类型、分页行数据、耗时与是否截断 | `connection_id` 或 `sqlite_path`, `sql`, `params?`, `limit?` (1..1000, 默认 200), `offset?` |
| `db__explain` | 返回 SQL 的执行计划（SQLite `EXPLAIN QUERY PLAN` / MySQL `EXPLAIN FORMAT=JSON` / Postgres `EXPLAIN (FORMAT JSON)`）用于慢查询诊断 | `connection_id` 或 `sqlite_path`, `sql`, `analyze?`（默认 `false`，`prod` 禁止 `analyze: true`） |
| `db__execute` | 执行写操作（`INSERT`/`UPDATE`/`DELETE`/DDL），受 `read_only` 与 `prod` 二次确认门禁保护，返回 `rows_affected` 与耗时 | `connection_id` 或 `sqlite_path`, `sql`, `params?`, `confirm_prod?: boolean` |
| `db__export_xlsx` | 执行只读查询并将结果直接导出为工作区内的标准 `.xlsx` 文件（复用 `rust-excel`，带表头样式、冻结首行、自动列宽），方便直接交给 Excel 插件查看或交付 | `connection_id` 或 `sqlite_path`, `sql`, `output_path`（工作区相对 `.xlsx`）, `sheet_name?`, `max_rows?` (默认 50000), `overwrite?` |
| `db__import_xlsx` | 读取工作区内 `.xlsx` 指定工作表，预览推断的列类型或直接创建表并批量事务导入数据 | `connection_id` 或 `sqlite_path`, `xlsx_path`, `sheet?`, `table_name`, `mode` (`preview\|create_and_insert\|append`), `confirm_prod?` |

> **便捷免配置体验**：所有查询/检视工具均支持二选一：`connection_id`（已保存的 MySQL/Postgres/SQLite 连接）**或** `sqlite_path`（工作区相对路径如 `data/app.db`，无需先建连接即可直接开查！）。

---

## 4. `db-server` HTTP API 契约（供 DB Studio 前端调用）

所有接口挂在 `/api/` 下（完全适配 QuickAgent `local-api` 反向代理与独立开发模式），监听 `BIND` 环境变量（默认 `127.0.0.1:0` 或 `127.0.0.1:41902`）：

| 方法与路径 | 说明 |
| :--- | :--- |
| `GET /api/health` | 返回 `{"ok":true,"engine":"rust-db","drivers":["sqlite","mysql","postgres"]}` |
| `GET /api/connections?workspace_root=...` | 返回脱敏连接列表 + 工作区发现的 SQLite 文件 |
| `POST /api/connections` | 新增/更新连接 |
| `POST /api/connections/test` | 测试连接连通性与服务端版本（不落盘也可测试） |
| `DELETE /api/connections/{id}` | 删除连接并清理连接池缓存 |
| `GET /api/schema?connection_id=...&sqlite_path=...&workspace_root=...&schema=...&table=...` | 获取数据库/表树或单表详细列/索引/外键/DDL |
| `POST /api/query` | 执行只读 SQL（支持分页 `limit`/`offset`），返回 `columns`, `rows`, `elapsed_ms`, `truncated`, `editable_table?`（当单表有主键时返回主键列信息以启用网格行内编辑！） |
| `POST /api/explain` | 获取查询执行计划 |
| `POST /api/execute` | 执行写 SQL / 批量单元格修改生成的参数化 `UPDATE`/`INSERT`/`DELETE`（校验 `read_only` 与 `prod` 确认） |
| `POST /api/xlsx/export` | 将当前查询结果导出为工作区 `.xlsx` 文件，或返回 `.xlsx` 二进制供浏览器直接下载 |
| `POST /api/xlsx/import` | 上传或指定工作区 `.xlsx` 预览列推断并导入数据库 |

---

## 5. 前端 DB Studio 工作台（`packages/db-studio/`）

严格保持与 `excel-studio` 和 QuickAgent 桌面端一致的视觉语言（支持 Light Paper / Dark Ink 自动跟随 `data-ds-dark-theme` 或 URL 参数 `theme=dark|light`，**零外部 CDN 资源**）：

1. **三区布局（Navicat 轻量版手感）**：
   - **左侧栏（260px，可折叠）**：
     - 顶部「+ 新建连接」按钮（支持 SQLite / MySQL / PostgreSQL，带环境徽章 `DEV` 蓝 / `STAGING` 橙 / `PROD` 红 + 只读锁图标）；
     - 「工作区 SQLite」快速发现分组（一键打开项目里的 `.db`/`.sqlite` 文件）；
     - 展开连接后的 **Schema / 表 / 视图树**（支持搜索过滤，点击表名直接打开「数据预览」Tab，右键或图标按钮查看「表结构 / DDL」）。
   - **右侧主区（上下可拖拽分栏）**：
     - **顶部多 Tab 栏**：支持同时开多个「表数据 Tab」和「SQL 查询 Tab」；
     - **上半区：SQL 编辑器 + 工具栏**：
       - 运行（`Ctrl/Cmd + Enter`，支持选中部分 SQL 运行）、`EXPLAIN`、格式化、只读/读写状态指示、**`PROD` 红色警示条**、执行耗时与行数徽章；
       - 轻量本地查询历史下拉（存 `localStorage`）。
     - **下半区：Excel 同款高性能结果网格 + 结构/计划子标签**：
       - 子标签 1：**数据网格**（复用 `excel-studio` 网格视觉与列宽拖拽/单元格选区/复制）：
         - `NULL` 显示柔和斜体徽章、长 JSON/文本点击弹出完整查看器；
         - 当查询来自**带主键的单表**且连接非只读时，支持**双击单元格直接行内编辑**，修改的行高亮标黄，底部出现「预览 SQL 并提交（N 处变更）」/「放弃」条——点击提交前弹窗展示实际要执行的 `UPDATE` 语句（`prod` 需二次确认）！
         - 顶栏一键 **「导出 Excel (.xlsx)」**（导出到工作区或浏览器下载）与 **「从 Excel 导入」** 按钮；
         - 底部分页栏（上一页/下一页/每页条数 50/200/500）。
       - 子标签 2：**表结构 / DDL**（列类型、是否可空、主键/外键/索引图标、一键复制 `CREATE TABLE` DDL）。
       - 子标签 3：**执行计划（EXPLAIN）**（结构化树/卡片展示全表扫描警告与索引命中情况）。
2. **QuickAgent 嵌入兼容**：
   - 参照 `packages/excel-studio/src/quickagent-preview.ts`，兼容 QuickAgent 传入的 `workspace_root`、`file`（打开 `.sqlite`/`.db` 附件时自动加载该 SQLite 库！）、`theme` 与同源 `local-api` 基础路径。

---

## 6. 分阶段施工与自测验收清单

### 阶段 1：Rust 引擎与 `db-server`（`rust-db/`）
- 完成 `sql_guard.rs`、`config.rs`（`0600` 权限与脱敏）、`drivers/{sqlite,mysql,postgres}.rs`、`xlsx_bridge.rs`、`server.rs`、`tool_rpc.rs`。
- 编写 `rust-db/tests/db_engine.rs` 并确保 `cargo test --manifest-path rust-db/Cargo.toml` 全量通过、0 warning：
  - SQLite 建表、插入、只读查询、分页、类型序列化（整数/浮点/文本/NULL/BLOB）；
  - `sql_guard` 单元测试：拦截 `DROP`、`DELETE`、`UPDATE`、`INSERT`、多语句 `SELECT 1; DROP TABLE x`、注释绕过 `/* comment */ DELETE ...`，放行 `SELECT`、`WITH cte AS (...) SELECT`、`EXPLAIN`、`PRAGMA table_info(...)`；
  - `read_only` 连接拒绝 `db__execute`；`prod` 连接未传 `confirm_prod: true` 拒绝 `db__execute`；
  - `connections.json` 权限为 `0600` 且返回体永不泄露明文密码；
  - `db__export_xlsx` 导出标准 `.xlsx` 且可被 `rust-excel` 正确读回；`db__import_xlsx` 预览与建表导入往返通过；
  - 路径穿越攻击（`../../etc/passwd`）被拦截。
- 验证现有 `cargo test --manifest-path rust-excel/Cargo.toml` 零回归。

### 阶段 2：DB Studio 前端（`packages/db-studio/`）
- 搭建 Vite + TypeScript 项目并复用 `excel-studio` 的网格与双主题样式。
- 通过 `npm run typecheck --workspace db-studio` 与 `npm run build --workspace db-studio`，且现有 `npm run typecheck && npm run build`（`excel-studio`）零回归。

### 阶段 3：QuickAgent 打包、自检与 Playwright 验收
- 完成 `scripts/quickagent-db/{manifest.mjs, SKILL.md, verify.mjs}` 与 `scripts/bundle-quickagent-db.sh`；
- 运行 `bash scripts/bundle-quickagent-db.sh` 将插件安装到 `/home/goodtime/Documents/project/codex-workspace-mcp/bundled_plugins/db`（要求 `verify.mjs` 自动校验 manifest 严格无未知字段、离线静态资源、`GET /api/health`、以及通过 stdio RPC 完成 SQLite 建表 → 查询 → 导出 `.xlsx` 全闭环）；
- 运行 `node scripts/browser-smoke-db.mjs`（Playwright），在 Light 与 Dark 双主题下验证：打开临时 SQLite 库 → 浏览 Schema 树 → 执行 SQL 查询 → 网格双击编辑并提交更新 → 导出 `.xlsx` → 截图保存到 `artifacts/browser/db-light.png` 和 `db-dark.png`，控制台 0 报错。

### 完成后向 Antigravity 汇报
1. 新增与修改文件清单；
2. `rust-db` / `rust-excel` 测试、`db-studio` / `excel-studio` 构建、`bundle-quickagent-db.sh` + `verify.mjs`、`browser-smoke-db.mjs` 的真实输出摘要；
3. 打包后的 `bundled_plugins/db` 二进制大小与任何待审查风险点。
