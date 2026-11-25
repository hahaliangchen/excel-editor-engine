# Excel Editor Engine

纯 Rust 的 .xlsx 服务与离线电子表格工作台。支持跨表公式计算、样式与合并单元格，以及 QuickAgent 自带的 8 个工作簿工具。架构和验收标准见 [docs/PLAN.md](docs/PLAN.md)。

## 运行

需要 Rust 1.90+、Node.js 22+ 及 npm。先在仓库根目录运行 npm ci，再分别启动：

~~~sh
npm run backend
npm run dev
~~~

打开 Vite 打印的本地地址。默认后端为 http://127.0.0.1:8083；也支持 BIND=127.0.0.1:端口 或 BIND=127.0.0.1 配合 PORT。独立前端固定连接 8083；宿主前端走同源 local-api。发布构建只包含本地 JS、CSS 和 SVG，无运行时外网依赖。

## 验收

~~~sh
RUSTFLAGS=-Dwarnings cargo test --locked --manifest-path rust-excel/Cargo.toml
npm run typecheck
npm test
npm run build
npm run preview -- -- --port 4183 --strictPort
~~~

在后端与生产预览都启动后，运行 npm run test:browser。默认使用本机 Google Chrome（可用 PLAYWRIGHT_CHANNEL 指定其他 Chromium channel），URL 可经 STUDIO_URL 覆盖；产物位于 artifacts/browser/，包含两种主题、窄屏截图及往返工作簿。测试监控浏览器错误、HTTP 失败、外部来源及首次装配时独立 CSS 的可用性。

## QuickAgent

~~~sh
scripts/bundle-quickagent.sh
~~~

默认安装到 /home/goodtime/Documents/project/codex-workspace-mcp/bundled_plugins/excel，可传绝对目标路径（目录名须为 excel）覆盖默认值。脚本发布前用实际二进制检查 BIND、健康检查和 stdio 创建/读取。宿主会面向当前工作区传入 workspace_root；工具 path 始终相对根目录，不跟随符号链接。读区、搜索和检查分页回包上限为 200 KiB，超长项可按 UTF-8 字节偏移续读。

## 兼容边界

支持普通 .xlsx 工作表，不支持 .xls、.xlsm 或加密文件。公式语法与函数以计划清单为准，不是完整 Excel 计算引擎。普通数据修改可保留源包中未受影响的 OOXML 部件；格式或结构改动使用 umya-spreadsheet 原工作簿为底稿。工作表、单元格样式、宽度、合并区域和公式缓存有往返测试；未验证第三方扩展、动态数组、宏、外部计算链或所有图表对象的无损重写。

## 数据库工作台

数据库工作台与 Excel 分开构建和打包。方案见 [PLAN_DB_PLUGIN.md](docs/PLAN_DB_PLUGIN.md)。依赖安装仍使用仓库根目录的 npm ci。分别从根目录启动：

~~~sh
npm run backend:db
npm run dev --workspace db-studio
~~~

打开 http://127.0.0.1:41903/，可通过网址参数 workspace_root 选择工作区；不传时服务使用启动目录，也可设置 DB_STUDIO_WORKSPACE_ROOT。Rust 服务默认监听 127.0.0.1:41902；若经 BIND 修改端口，Vite 侧用 DB_STUDIO_BACKEND 指向新地址。QuickAgent 中使用同源 local-api，无需 Vite。

连接档案默认存于 ~/.config/grow-db/connections.json，或使用 DB_STUDIO_DATA_DIR 指定仓库外的私有目录。目录权限为 0700，档案为 0600；不要提交它们。生产库应使用按需授权的账号并启用验证 TLS；数据库原生用户函数和远端权限由数据库管理员管理。SQLite 绝对路径必须位于工作区内，路径和 SQLite 辅助文件不应由不可信进程并发更换。QuickAgent 附件以只读快照打开，不包含单独的 WAL 文件。

~~~sh
cargo test --locked --manifest-path rust-db/Cargo.toml
npm run typecheck --workspace db-studio
npm run build --workspace db-studio
node scripts/browser-smoke-db.mjs
bash scripts/bundle-quickagent-db.sh
~~~

Playwright 脚本启动真实 Rust 服务和 QuickAgent 风格的静态页代理，在临时 SQLite 文件上运行，截图及往返工作簿写入 artifacts/browser/。打包默认安装到 /home/goodtime/Documents/project/codex-workspace-mcp/bundled_plugins/db，同目录验包后原子安装；重新安装要求文件系统支持原子目录交换。SQLite 已覆盖端到端路径；远程数据库的 DDL（PostgreSQL 表结构为重建预览）与权限需在部署环境复验。MySQL 的 DDL 隐式提交，建表导入失败时会尝试清理已创建的表。
