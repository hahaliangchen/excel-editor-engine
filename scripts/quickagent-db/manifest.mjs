import {writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const text=description=>({type:'string',description});
const flag=description=>({type:'boolean',description});
const choice=(...values)=>({type:'string',enum:values});
const integer=(minimum,maximum)=>({type:'integer',minimum,maximum});
const target={connection_id:text('已保存连接 ID'),sqlite_path:text('工作区内 SQLite 文件路径；与 connection_id 二选一')};
const oneTarget={oneOf:[{required:['connection_id']},{required:['sqlite_path']}]};
const pagination={limit:integer(1,1000),offset:integer(0,10000000),fragment_offset:integer(0,8388608)};
const parameters={sql:text('单条语句'),params:{type:'array',items:{}}};
const tool=(suffix,description,properties,required=[],extra={})=>({name:'db__'+suffix,description,input_schema:{type:'object',properties,required,additionalProperties:false,...extra},program:'bin/db-server',args:['--tool','db__'+suffix],timeout_seconds:60});
const tools=[
  tool('list_connections','列出脱敏连接及工作区内 SQLite 文件。',{include_workspace_sqlite:flag('是否发现工作区根目录的 SQLite 文件'),...pagination}),
  tool('save_connection','保存连接档案；可测试连通性，仅返回脱敏信息。',{id:text('连接 ID'),name:text('名称'),driver:choice('sqlite','mysql','postgres'),env:choice('dev','staging','prod'),read_only:flag('只读锁'),host:text('主机'),port:integer(1,65535),database:text('数据库'),username:text('用户名'),password:text('密码；留空保留已有值'),ssl_mode:choice('disabled','preferred','required','verify_ca','verify_full'),file_path:text('工作区内 SQLite 文件路径'),notes:text('备注'),test_connection:flag('测试连通性')},['name','driver','env']),
  tool('inspect_schema','浏览架构、表和视图，或检查列、主键、索引、外键及 DDL。',{...target,schema:text('架构名'),table:text('表名'),include_ddl:flag('返回 DDL'),...pagination},[],oneTarget),
  tool('query','执行单条只读 SQL；使用 next_offset 或 next_fragment_offset 分页。默认仅在后台返回给 AI 做分析，不抢占用户当前打开的表标签；仅当用户明确要求在界面展示筛选/查询结果时传 show_in_ui: true。',{...target,...parameters,...pagination,show_in_ui:flag('是否同步展示到数据库工作台的「AI 查询」标签页；默认 false（后台统计、计数或抽样探针不打扰用户当前表）。仅当用户明确要求在界面查看该过滤/查询结果时传 true')},['sql'],oneTarget),
  tool('explain','返回查询执行计划；生产环境禁用 ANALYZE。',{...target,...parameters,...pagination,analyze:flag('执行并统计运行时间')},['sql'],oneTarget),
  tool('execute','执行 DML/DDL；只读连接禁止写入，生产环境必须明确确认。',{...target,...parameters,confirm_prod:flag('已经用户确认生产写入')},['sql'],oneTarget),
  tool('export_xlsx','把只读查询结果导出到工作区内标准 XLSX。',{...target,...parameters,output_path:text('工作区内输出 .xlsx 路径'),sheet_name:text('工作表名称'),max_rows:integer(1,50000),overwrite:flag('覆盖已有文件')},['sql','output_path'],oneTarget),
  tool('import_xlsx','从工作区内 XLSX 预览、建表并导入或追加。',{...target,xlsx_path:text('工作区内 .xlsx 路径'),sheet:text('工作表名称，默认首张'),schema:text('目标架构'),table_name:text('目标表'),mode:choice('preview','create_and_insert','append'),confirm_prod:flag('已经用户确认生产写入'),...pagination},['xlsx_path','table_name','mode'],{allOf:[{if:{properties:{mode:{enum:['create_and_insert','append']}}},then:oneTarget}]}),
  tool('focus_view','像人工点选左侧架构树与表格一样直接操作数据库工作台 UI：自动展开并选中左侧目标表、打开完整数据表标签页（保留全部列与前后相邻行），并高亮框选指定整列（仅传 column）、整行（仅传 row）、单元格或 A1 区域并滚动居中；大表定位非首页记录时可传 offset 跳到目标所在分页并保留上下文行。只要能靠 UI 定位看到的，优先调用本工具，不要用单列/单行 SQL 覆盖整表。',{...target,schema:text('可选架构名'),table:text('可选表名（支持 schema.table）；传入时在左侧树选中并打开该完整表标签页，省略时作用于当前标签页'),column:text('可选列名（或列字母/1-based 列号）；仅传 column 时高亮选中整列并横向居中滚动'),row:integer(1,1000000),offset:integer(0,10000000),range:text('可选 A1 选区，如 F:F、25:25 或 B5:D10'),find_column:text('可选：配合 find_value 指定搜索列'),find_value:text('可选：在目标分页按子串定位首条匹配行并滚动高亮')})
];
export const manifest={version:1,id:'db',library:'db',title:'数据库',enabled_by_default:true,skill:'SKILL.md',pages:[{id:'studio',entry:'page/index.html',title:'数据库',renderer:'html'}],local_service:{program:'bin/db-server'},attachment_extensions:['db','sqlite','sqlite3'],tools:tools.map(t=>t.name),command_tools:tools};
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)&&process.argv[2])await writeFile(process.argv[2],JSON.stringify(manifest,null,2)+'\n');
