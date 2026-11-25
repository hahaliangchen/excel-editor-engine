import {writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const text=(description,extra={})=>({type:'string',description,...extra});
const num=(description,extra={})=>({type:'number',description,...extra});
const integer=(description,minimum=1,maximum)=>({type:'integer',description,minimum,...(maximum===undefined?{}:{maximum})});
const bool=description=>({type:'boolean',description});
const object=(properties,required=[],extra={})=>({type:'object',properties,required,additionalProperties:false,...extra});
const oneOf=(...schemas)=>({oneOf:schemas});
const action=(name,properties,required)=>object({action:{const:name},...properties},['action',...required]);
const path=text('工作区相对路径，必须以 .xlsx 结尾；不支持绝对路径或符号链接');
const sheet=text('工作表名称');
const range=text('A1 区域，如 A1:H40');
const name=text('工作表名称，最多 31 个 UTF-16 字符');
const pages={start_index:integer('分页起始位置（从 0 开始）',0),limit:integer('返回项上限（默认 100）',1,1000),fragment_offset:integer('超长项的 JSON 字符串续传 UTF-8 字节位置',0)};
const nullable=s=>oneOf(s,{type:'null'});
const style=object({
  bold:bool('粗体'),italic:bool('斜体'),color:nullable(text('字体颜色 #RRGGBB；null 清除')),
  fill:nullable(text('背景填充 #RRGGBB；null 清除')),align:nullable({type:'string',enum:['left','center','right']}),
  numberFormat:nullable(text('Excel 格式代码，如 ¥#,##0.00 或 0.0%；null 重置')),
});
const tool=(suffix,description,properties,required=[])=>({
  name:'excel__'+suffix,description,program:'bin/excel-server',args:['--tool'],
  input_schema:object({path,...properties},['path',...required]),
});
const raw=oneOf({type:'string',description:'文本或以 = 起始的公式；用单引号前缀保留文本类型'},{type:'number'},{type:'boolean'},{type:'null'});
const createCell=object({raw:text('单元格文本、数字或以 = 起始的公式'),style},['raw']);
const workbookSheet=object({
  name,cells:{type:'object',additionalProperties:createCell,description:'用大写 A1 地址作为键'},
  merges:{type:'array',items:range},widths:{type:'object',additionalProperties:num('Excel 列宽',{minimum:0,maximum:255})},frozen:bool('冻结首行'),
},['name']);
const tools=[
  tool('inspect','分页查看工作表名称、尺寸、合并数、公式数及前 3 行预览。',pages),
  tool('read_range','按行优先分页读取 A1 范围，返回 raw、formula、value、display 和样式。', {sheet,range,...pages},['sheet','range']),
  tool('write_cells','批量写入单元格，重算跨表依赖，保存并返回写入值及部分公式结果。', {sheet,cells:{type:'object',additionalProperties:raw,description:'A1 地址到数值、布尔、文本或公式的映射'}},['sheet','cells']),
  tool('manage_rows_cols','插入、删除行列，设置列宽或冻结首行；同步调整公式引用。',{sheet,change:oneOf(
    ...['insert_rows','delete_rows','insert_columns','delete_columns'].map(op=>action(op,{at:integer('起始行或列（从 1 开始）'),count:integer('数量，默认 1')},['at'])),
    action('set_width',{column:integer('列号',1,16384),width:num('Excel 列宽',{minimum:0,maximum:255})},['column','width']),
    action('freeze_first_row',{frozen:bool('是否冻结')},['frozen']),
  )},['sheet','change']),
  tool('manage_sheets','新建、重命名、复制或删除工作表；同步调整跨表引用。',{change:oneOf(
    action('new',{name},['name']),action('rename',{sheet,name},['sheet','name']),action('copy',{sheet,name},['sheet','name']),action('delete',{sheet},['sheet']),
  )},['change']),
  tool('format_range','格式化 A1 范围，可合并或取消合并；合并不会隐藏非空值。',{sheet,range,style,merge:bool('true 合并，false 取消合并')},['sheet','range']),
  tool('search','分页查找计算值或公式，可指定工作表或区分大小写。',{sheet,query:text('查找文本'),case_sensitive:bool('区分大小写'),...pages},['query']),
  tool('create_xlsx','从结构化多表数据、样式和公式生成标准 XLSX，默认不覆盖文件。',{
    overwrite:bool('允许覆盖已存在的工作簿'),workbook:object({sheets:{type:'array',items:workbookSheet,minItems:1}},['sheets']),
  },['workbook']),
  {
    name:'excel__focus_view',
    description:'在左侧 Excel 工作台视图中切换工作表、选中指定列/行/单元格或区域并自动滚动到视野内。',
    program:'bin/excel-server',
    args:['--tool'],
    input_schema:object({
      path:text('可选工作区相对 .xlsx 路径'),
      sheet:text('可选工作表名称'),
      range:text('可选 A1 选区，如 F:F、25:25、F25 或 B5:D10'),
      column:text('可选列字母（如 F）、1-based 列号或首行表头名称（如 日报内容）；仅传 column 时选中整列并横向滚到该列'),
      row:integer('可选 1-based 行号；仅传 row 时选中整行并纵向滚到该行',1,1048576),
      find_column:text('可选：配合 find_value 限定搜索列（列字母或表头名）'),
      find_value:text('可选：在当前工作表按子串定位首条匹配行并滚动选中'),
    }),
  },
];

export const manifest={
  version:1,id:'excel',library:'excel',title:'Excel 表格',enabled_by_default:true,
  skill:'SKILL.md',pages:[{id:'studio',title:'Excel 表格',entry:'page/index.html',renderer:'html'}],
  local_service:{program:'bin/excel-server'},attachment_extensions:['xlsx'],
  tools:tools.map(t=>t.name),command_tools:tools,
};
if(process.argv[1] && resolve(process.argv[1])===fileURLToPath(import.meta.url) && process.argv[2])await writeFile(process.argv[2],JSON.stringify(manifest,null,2)+'\n');
