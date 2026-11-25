import '../../excel-studio/src/styles/studio.css';
import './styles/studio.css';
import {Client,embed,type Connection,type Detail,type Env,type ImportInput,type Kind,type ResultPage,type Table} from './api-client';
import {base64,btn,download,el,field,heading,input,modal,select} from './components/controls';
import {ResultGrid,text} from './grid';
import {formatSql,mark,remember,savedHistory,suggestions} from './sql-editor';
import {quote,Store,type Source,type Tab} from './store';

const api=new Client(), store=new Store();store.open();
const params=new URLSearchParams(location.search);
const app=document.querySelector<HTMLDivElement>('#app')!;
const shell=el('div','db-shell');app.append(shell);
const top=el('header','db-top');
const brand=el('div','db-brand');const logo=el('img');logo.src=new URL('./db.svg',document.baseURI).href;logo.alt='';
const titles=el('div');titles.append(el('h1','','数据库工作台'),el('span','db-brand-sub','DATABASE STUDIO'));brand.append(logo,titles);
const topStatus=el('div','db-workspace');const rootName=el('span','db-root-name');topStatus.append(el('span','db-dot'),el('span','','本地服务'),rootName);
const themeButton=btn('切换主题','moon',false);top.append(brand,topStatus,themeButton);shell.append(top);
const layout=el('div','db-layout');shell.append(layout);
const side=el('aside','db-side');const menu=el('header','db-side-head');const addConnection=btn('新建连接','plus');const reload=btn('刷新连接','refresh-cw',false);const collapse=btn('折叠连接栏','panel-top',false);
menu.append(heading('连接与架构'),addConnection,reload,collapse);const filterTree=input('搜索架构','search');filterTree.placeholder='搜索连接、表…';
const sidebarFilter=el('div','db-side-filter');sidebarFilter.append(filterTree);const tree=el('nav','db-tree');tree.setAttribute('aria-label','数据库架构');side.append(menu,sidebarFilter,tree);
const work=el('main','db-work');layout.append(side,work);
const tabs=el('nav','db-tabs');tabs.setAttribute('aria-label','查询标签页');const tabItems=el('div','db-tab-items');const addTab=btn('新建查询','plus',false);const expand=btn('展开连接栏','panel-top',false);expand.hidden=true;tabs.append(expand,tabItems,addTab);work.append(tabs);
const editorPane=el('section','db-editor-pane');const editorTools=el('header','db-tools db-editor-tools');
const runButton=btn('运行查询','arrow-down',true,'db-primary');runButton.title='运行查询（Ctrl/Cmd + Enter）';
const writeButton=btn('执行写入','triangle-alert');const explainButton=btn('执行计划','sigma');const formatButton=btn('格式化 SQL','rows-3',false);
const history=select('查询历史',[['','查询历史']]);const sourceSelect=select('当前连接',[['','选择连接']]);const sessionState=el('span','db-session');
editorTools.append(runButton,writeButton,explainButton,formatButton,history,sourceSelect,sessionState);
const prod=el('div','db-prod','生产环境 · 写入需要二次确认');prod.hidden=true;
const editorFrame=el('div','db-editor-frame');const gutter=el('pre','db-gutter','1');gutter.setAttribute('aria-hidden','true');
const codewrap=el('div','db-codewrap');const highlight=el('pre','db-highlight');highlight.setAttribute('aria-hidden','true');
const editor=el('textarea','db-editor');editor.setAttribute('aria-label','SQL 编辑器');editor.spellcheck=false;editor.wrap='off';editor.placeholder='SELECT …';
const completions=el('div','db-completion');completions.hidden=true;codewrap.append(highlight,editor,completions);editorFrame.append(gutter,codewrap);editorPane.append(editorTools,prod,editorFrame);
const split=el('div','db-split');split.setAttribute('role','separator');split.setAttribute('aria-label','调整 SQL 编辑器高度');split.setAttribute('aria-orientation','horizontal');split.tabIndex=0;split.append(el('span'));
const result=el('section','db-result');const resultsHead=el('header','db-tools db-results-head');const dataView=btn('结果数据','table-2');const structureView=btn('表结构 / DDL','columns-3');const planView=btn('EXPLAIN','sigma');
const views=el('div','db-view-tabs');views.append(dataView,structureView,planView);const count=el('span','db-result-count');resultsHead.append(views,count);
const resultsTools=el('div','db-tools db-results-tools');const filterGrid=input('筛选当前页','search');filterGrid.placeholder='筛选当前页';const exportButton=btn('导出 Excel (.xlsx)','download');const importButton=btn('从 Excel 导入','folder-open');
const askAiButton=btn('问 AI',undefined,true,'db-primary');askAiButton.title='把当前连接、SQL 与选中/可见数据带给右侧 AI 助手';
{const sp=el('span','icon');sp.setAttribute('aria-hidden','true');sp.innerHTML='<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M10 2.5c.6 3.9 2.6 5.9 6.5 6.5-3.9.6-5.9 2.6-6.5 6.5-.6-3.9-2.6-5.9-6.5-6.5 3.9-.6 5.9-2.6 6.5-6.5Z"/><path d="M18.5 13.5c.3 1.9 1.2 2.8 3 3-1.8.3-2.7 1.1-3 3-.3-1.9-1.2-2.7-3-3 1.8-.2 2.7-1.1 3-3Z" opacity=".75"/></svg>';askAiButton.prepend(sp);}
resultsTools.append(filterGrid,exportButton,importButton,askAiButton);
const viewer=el('div','db-viewer');const gridWrap=el('div','db-grid-wrap');const blank=el('div','db-empty','暂无查询结果');
const rich=el('div','db-rich');const grid=new ResultGrid(()=>renderResult(false),showText,report,()=>scheduleContext(150));gridWrap.append(grid.root,blank);viewer.append(gridWrap,rich);
const patchBar=el('div','db-patch');const patchCount=el('span');const submit=btn('预览 SQL 并提交','check',true,'db-primary');const discard=btn('放弃修改','x');patchBar.append(patchCount,submit,discard);
const resultFoot=el('footer','db-result-foot');const dataset=el('span','db-dataset');const prev=btn('上一页','arrow-up',false);const next=btn('下一页','arrow-down',false);const range=el('span','db-range');const limit=select('每页条数',[['50','50 条 / 页'],['200','200 条 / 页'],['500','500 条 / 页']]);resultFoot.append(dataset,prev,range,next,limit);
result.append(resultsHead,resultsTools,viewer,patchBar,resultFoot);work.append(editorPane,split,result);
const footer=el('footer','db-status');const job=el('span','db-job','就绪');job.setAttribute('role','status');footer.append(job,el('span','db-status-engine','RUST ENGINE'));shell.append(footer);

let pending=0;
const expansions=new Set<string>();
const loading=new Set<string>();
function status(message:string,error=false){job.textContent=message;job.classList.toggle('db-error',error);}
function report(error:unknown){status(error instanceof Error?error.message:String(error),true);}
async function task(action:()=>Promise<void>){pending++;job.setAttribute('aria-busy','true');try{await action();}catch(e){report(e);}finally{if(--pending===0)job.removeAttribute('aria-busy');}}
function bind(b:HTMLButtonElement,fn:()=>Promise<void>|void){b.onclick=()=>void task(async()=>fn());}
function confirmDiscard(tab:Tab){if(!tab.patches.size)return true;if(!confirm('放弃当前结果尚未提交的修改？'))return false;tab.patches.clear();return true;}
function sourceOf(conn:Connection,old?:Source):Source {return {key:'conn:'+conn.id,name:conn.name,target:{connection_id:conn.id},connection:conn,readOnly:conn.read_only,tables:old?.tables??new Map(),schemas:old?.schemas,schema:old?.schema};}
function local(path:string,old?:Source):Source {return old??{key:'file:'+path,name:path,target:{sqlite_path:path},readOnly:false,schema:'main',tables:new Map()};}
function addSource(s:Source){if(!store.sources.some(x=>x.key===s.key))store.sources.push(s);renderTree();renderSession();return store.sources.find(x=>x.key===s.key)!;}
function setTheme(theme:string,keep=false){if(theme!=='dark'&&theme!=='light')return;document.documentElement.dataset.theme=theme;if(keep)localStorage.setItem('db-studio-theme',theme);themeButton.innerHTML=btn('切换主题',theme==='dark'?'sun':'moon',false).innerHTML;}
setTheme(document.documentElement.dataset.theme??'light');themeButton.onclick=()=>setTheme(document.documentElement.dataset.theme==='dark'?'light':'dark',true);
function renderSession(){
  const t=store.tab,s=t.source;sourceSelect.replaceChildren(new Option('选择连接',''),...store.sources.map(x=>new Option(x.name,x.key)));sourceSelect.value=s?.key??'';
  prod.hidden=s?.connection?.env!=='prod';sessionState.textContent=s?(s.readOnly?'只读':'读写'):'未连接';sessionState.dataset.active=String(!!s);
  runButton.disabled=explainButton.disabled=!s;writeButton.disabled=!s||s.readOnly;askAiButton.disabled=!s&&!t.page;rootName.textContent=api.workspace.split('/').filter(Boolean).pop()??'';
  scheduleContext(100);
}
function renderTabs(){
  tabItems.replaceChildren();for(const t of store.tabs){const wrap=el('div','db-tab');wrap.classList.toggle('active',t.id===store.active);
    const b=btn(t.label,t.table?'table-2':'columns-3');b.setAttribute('role','tab');b.setAttribute('aria-selected',String(t.id===store.active));b.onclick=()=>{store.active=t.id;renderTab();};
    const close=btn('关闭 '+t.label,'x',false);close.onclick=()=>{if(confirmDiscard(t)){store.close(t);renderTab();}};wrap.append(b,close);tabItems.append(wrap);
  }
}
function renderText(){mark(editor.value,highlight);gutter.textContent=editor.value.split('\n').map((_,i)=>i+1).join('\n');scrollEditor();}
function scrollEditor(){highlight.scrollTop=editor.scrollTop;highlight.scrollLeft=editor.scrollLeft;gutter.scrollTop=editor.scrollTop;}
function renderHistory(){history.replaceChildren(new Option('查询历史',''),...savedHistory().map(s=>new Option(s.replace(/\s+/g,' ').slice(0,85),s)));}
function renderTab(){renderTabs();renderSession();const t=store.tab;editor.value=t.sql;renderText();renderHistory();completions.hidden=true;renderTree();renderResult(true);}
function renderResult(reset=true){
  const t=store.tab,p=t.page;
  for(const [button,v] of [[dataView,'data'],[structureView,'structure'],[planView,'plan']] as const)button.setAttribute('aria-pressed',String(t.view===v));
  structureView.disabled=!t.detail&&!t.table;planView.disabled=!t.plan;
  gridWrap.hidden=t.view!=='data';rich.hidden=t.view==='data';blank.hidden=!!p;grid.root.hidden=!p;
  resultsTools.hidden=t.view!=='data';resultFoot.hidden=t.view!=='data';patchBar.hidden=!t.patches.size||t.view!=='data';
  if(reset){filterGrid.value='';grid.load(t);}
  exportButton.disabled=!p||!t.executed;importButton.disabled=!t.source||t.source.readOnly;filterGrid.disabled=!p;
  count.textContent=p?p.rows.length+' 行 · '+p.elapsed_ms+' ms':'';
  dataset.textContent=p?(filterGrid.value?grid.count+' / ':'')+p.rows.length+' 行 · '+p.columns.length+' 列':'0 行';
  range.textContent=p?(p.rows.length?(p.offset+1)+'–'+(p.offset+p.rows.length):'0'):'—';prev.disabled=!p?.offset;next.disabled=!p?.truncated;limit.value=String(t.limit);
  patchCount.textContent=t.patches.size+' 处未提交变更';
  if(t.view==='structure')renderStructure(t.detail);
  if(t.view==='plan')renderPlan(t.plan);
  scheduleContext(80);
}
function renderStructure(d?:Detail){
  rich.replaceChildren();if(!d){rich.append(el('div','db-empty','正在读取结构…'));return;}
  const h=el('header','db-detail-head');h.append(el('h3','',d.schema+'.'+d.table),el('span','db-chip',d.kind==='view'?'视图':'表'));rich.append(h);
  const table=el('table','db-detail-table');const head=el('thead');const hdr=el('tr');for(const label of ['列名','类型','约束','默认值'])hdr.append(el('th','',label));head.append(hdr);
  const rows=el('tbody');for(const c of d.columns){const row=el('tr');for(const v of [c.name,c.type,[c.primary_key?'主键':'',c.nullable?'可空':'非空',c.generated?'生成列':''].filter(Boolean).join(' · '),c.default??'—'])row.append(el('td','',v));rows.append(row);}table.append(head,rows);rich.append(table);
  for(const [name,items] of [['索引',d.indexes],['外键',d.foreign_keys]] as const){if(items.length){const block=el('details','db-detail-block');block.append(el('summary','',name+' · '+items.length),el('pre','',JSON.stringify(items,null,2)));rich.append(block);}}
  if(d.ddl){const block=el('section','db-detail-block');const label=el('header','db-detail-head');const copy=btn('复制 DDL','file-plus-2');bind(copy,async()=>{await navigator.clipboard.writeText(d.ddl!);status('已复制 DDL');});label.append(el('h3','','DDL'),copy);block.append(label,el('pre','',d.ddl));rich.append(block);}
}
function planNodes(value:unknown,inside:HTMLElement,depth=0){
  if(depth>16)return;
  if(Array.isArray(value)){for(const x of value)planNodes(x,inside,depth+1);return;}
  if(value&&typeof value==='object'){
    const v=value as Record<string,unknown>;const keys=Object.keys(v);
    const own=v['Node Type']??v.access_type??v.table_name;let parent=inside;
    if(own){const card=el('div','db-plan-node'+(/Seq Scan|^ALL$/i.test(String(own))?' db-scan':''));card.append(el('strong','',String(own)),el('span','',[v['Relation Name']??v.table_name,v['Index Name']??v.key,v['Plan Rows']!==undefined?v['Plan Rows']+' rows':undefined].filter(Boolean).join(' · ')));inside.append(card);parent=card;}
    for(const k of keys)if(v[k]&&typeof v[k]==='object')planNodes(v[k],parent,depth+1);
  }
}
function renderPlan(p?:ResultPage){rich.replaceChildren();if(!p)return;
  const head=el('header','db-detail-head');head.append(el('h3','','执行计划'),el('span','db-chip',p.elapsed_ms+' ms'));rich.append(head);
  for(const row of p.rows){const detail=row[row.length-1];let parsed:unknown=detail;if(typeof detail==='string'&&/^[\[{]/.test(detail.trim())){try{parsed=JSON.parse(detail);}catch{/* plain text */}}
    const section=el('div','db-plan-item');if(parsed&&typeof parsed==='object')planNodes(parsed,section);
    else{const line=text(detail);section.append(el('div','db-plan-node'+(/\bSCAN\b/i.test(line)&&!/USING.*INDEX/i.test(line)?' db-scan':''),line));}
    if(parsed&&typeof parsed==='object'){const raw=el('details','db-detail-block');raw.append(el('summary','','原始计划'),el('pre','',JSON.stringify(parsed,null,2)));section.append(raw);}rich.append(section);
  }
}
function showText(title:string,value:string){const m=modal(title);m.body.append(el('pre','db-value',value));const copy=btn('复制','file-plus-2');bind(copy,async()=>{await navigator.clipboard.writeText(value);status('已复制');});m.foot.append(copy);m.open();}
function renderTree(){
  const needle=filterTree.value.trim().toLocaleLowerCase();tree.replaceChildren();
  for(const [name,sources] of [['已保存连接',store.sources.filter(s=>s.connection)],['工作区 SQLite',store.sources.filter(s=>!s.connection)]] as const){
    const group=el('section','db-tree-group');const groupHead=el('header','db-tree-group-head');groupHead.append(heading(name),el('span','db-tree-count',String(sources.length)));group.append(groupHead);
    for(const s of sources){
      const tables=[...s.tables.values()].flat();if(needle&&!s.name.toLocaleLowerCase().includes(needle)&&!tables.some(t=>t.name.toLocaleLowerCase().includes(needle)))continue;
      const row=el('div','db-tree-row');row.classList.toggle('current',store.tab.source?.key===s.key);
      const b=btn(s.name,expansions.has(s.key)?'chevron-down':'chevron-right');b.classList.add('db-tree-name');b.setAttribute('aria-expanded',String(expansions.has(s.key)));bind(b,()=>openSource(s,true));row.append(b);
      const env=el('span','db-env',s.connection?.env.toUpperCase()??(s.readOnly?'只读':'SQLite'));env.dataset.env=s.connection?.env??'local';if(s.readOnly)env.title='只读连接';row.append(env);
      if(s.connection){const edit=btn('编辑 '+s.name,'ellipsis',false);edit.classList.add('db-tree-aux');edit.onclick=()=>connectionDialog(s.connection);row.append(edit);}group.append(row);
      if(expansions.has(s.key)){
        if(loading.has(s.key))group.append(el('p','db-tree-note','读取架构…'));
        for(const name of s.schemas??[]){const k=s.key+'/'+name;const schemaRow=el('div','db-schema-row');const schemaBtn=btn(name,expansions.has(k)?'chevron-down':'chevron-right');schemaBtn.classList.add('db-tree-name');schemaBtn.setAttribute('aria-expanded',String(expansions.has(k)));bind(schemaBtn,()=>openSchema(s,name,true));schemaRow.append(schemaBtn);group.append(schemaRow);
          if(expansions.has(k)||needle){for(const t of s.tables.get(name)??[]){if(needle&&!s.name.toLocaleLowerCase().includes(needle)&&!t.name.toLocaleLowerCase().includes(needle))continue;group.append(tableRow(s,name,t));}}
        }
      }
    }
    if(!sources.length)group.append(el('div','db-tree-empty',name==='已保存连接'?'暂无连接':'未发现 SQLite 文件'));
    tree.append(group);
  }
}
function tableRow(s:Source,schema:string,t:Table){const row=el('div','db-table-row');const b=btn(t.name,t.kind==='view'?'panel-top':'table-2');b.classList.add('db-tree-name');bind(b,()=>openTable(s,schema,t,false));const info=btn('查看 '+t.name+' 结构','ellipsis',false);info.classList.add('db-tree-aux');bind(info,()=>openTable(s,schema,t,true));row.oncontextmenu=e=>{e.preventDefault();void task(()=>openTable(s,schema,t,true));};row.append(b,info);return row;}
async function loadSources(silent=false){
  const r=await api.list(),old=new Map(store.sources.map(s=>[s.key,s]));
  const fresh=[...r.connections.map(c=>sourceOf(c,old.get('conn:'+c.id))),...r.workspace_sqlite.map(x=>local(x.path,old.get('file:'+x.path)))];
  for(const s of old.values())if(s.key.startsWith('attachment:')||s.key.startsWith('file:')&&!fresh.some(x=>x.key===s.key))fresh.push(s);
  const sig=(list:Source[])=>list.map(s=>`${s.key}|${s.name}|${s.connection?.env??''}|${s.connection?.read_only??''}`).join('\n');
  if(silent&&sig(fresh)===sig(store.sources))return;
  store.sources=fresh;for(const t of store.tabs)if(t.source)t.source=fresh.find(s=>s.key===t.source?.key);
  if(store.selected)store.selected=fresh.find(s=>s.key===store.selected?.key);renderTree();renderSession();if(!silent)status('就绪');
}
async function openSource(s:Source,toggle=false){
  store.selected=s;const t=store.tab;
  if(!t.source || (!t.page&&!t.table&&!t.patches.size)) {t.source=s;t.detail=undefined;t.plan=undefined;renderSession();}
  if(toggle&&expansions.has(s.key)){expansions.delete(s.key);renderTree();return;}
  expansions.add(s.key);loading.add(s.key);renderTree();
  try{if(!s.schemas){const r=await api.schemas(s.target);s.schemas=r.schemas;s.schema=r.default_schema;}if(s.schema&&s.schemas.includes(s.schema))await openSchema(s,s.schema,false);}
  finally{loading.delete(s.key);renderTree();}
}
async function openSchema(s:Source,schema:string,toggle:boolean){const k=s.key+'/'+schema;if(toggle&&expansions.has(k)){expansions.delete(k);renderTree();return;}expansions.add(k);if(!s.tables.has(schema))s.tables.set(schema,await api.tables(s.target,schema));renderTree();}
async function openTable(s:Source,schema:string,table:Table,structure:boolean){
  const existing=store.tabs.find(t=>t.source?.key===s.key&&t.schema===schema&&t.table===table.name);
  if(existing){store.active=existing.id;existing.view=structure?'structure':'data';renderTab();if(structure&&!existing.detail)await detail(existing);else if(!structure&&!existing.page)await run(existing,0,existing.sql);return;}
  const q=(n:string)=>quote(s.connection?.driver??'sqlite',n),t=store.open({label:table.name,source:s,schema,table:table.name,sql:'SELECT * FROM '+q(schema)+'.'+q(table.name),view:structure?'structure':'data'});
  renderTab();if(structure)await detail(t);else await run(t,0,t.sql);
}
async function detail(t:Tab){if(!t.source||!t.table||!t.schema)return;t.detail=await api.detail(t.source.target,t.schema,t.table);if(t===store.tab)renderResult(false);}
function selectedSql(){return (editor.value.slice(editor.selectionStart,editor.selectionEnd).trim()||editor.value.trim());}
async function run(t:Tab,offset:number,sql:string){
  if(!t.source||!sql)throw new Error('请选择连接并输入 SQL');
  const source=t.source;status('正在执行查询…');const page=await api.query(source.target,sql,t.limit,offset);if(t.source!==source)return;
  t.page=page;t.executed=sql;t.view='data';t.patches.clear();
  if(page.editable_table&&!t.table){t.schema=page.editable_table.schema;t.table=page.editable_table.table;}
  remember(sql);if(t===store.tab){renderHistory();renderResult(true);}status('查询完成 · '+page.elapsed_ms+' ms');
}
let dbQueue=false;
function single(fn:()=>Promise<void>){return async()=>{if(dbQueue)return;dbQueue=true;try{await fn();}finally{dbQueue=false;}};}
bind(runButton,single(async()=>{const t=store.tab;if(confirmDiscard(t))await run(t,0,selectedSql());}));
bind(prev,single(async()=>{const t=store.tab;if(t.page?.offset&&confirmDiscard(t))await run(t,Math.max(0,t.page.offset-t.limit),t.executed);}));
bind(next,single(async()=>{const t=store.tab;if(t.page?.next_offset!=null&&confirmDiscard(t))await run(t,t.page.next_offset,t.executed);}));
limit.onchange=()=>void task(single(async()=>{const t=store.tab;const v=Number(limit.value);if(!confirmDiscard(t)){limit.value=String(t.limit);return;}t.limit=v;if(t.executed)await run(t,0,t.executed);}));
bind(explainButton,single(async()=>{const t=store.tab,sql=selectedSql();if(!t.source||!sql)throw new Error('请选择连接并输入 SQL');t.plan=await api.explain(t.source.target,sql);t.view='plan';if(t===store.tab)renderResult(false);status('执行计划已更新');}));
bind(structureView,async()=>{const t=store.tab;t.view='structure';renderResult(false);if(!t.detail)await detail(t);});dataView.onclick=()=>{store.tab.view='data';renderResult(false);};planView.onclick=()=>{store.tab.view='plan';renderResult(false);};
addTab.onclick=()=>{store.open();renderTab();editor.focus();};addConnection.onclick=()=>connectionDialog();bind(reload,async()=>{for(const s of store.sources){s.schemas=undefined;s.tables.clear();}await loadSources();await Promise.all(store.sources.filter(s=>expansions.has(s.key)).map(s=>openSource(s)));});
collapse.onclick=()=>{layout.classList.add('collapsed');expand.hidden=false;};expand.onclick=()=>{layout.classList.remove('collapsed');expand.hidden=true;};
filterTree.oninput=renderTree;filterGrid.oninput=()=>{grid.search(filterGrid.value);renderResult(false);};
sourceSelect.onchange=()=>{const t=store.tab;if(!confirmDiscard(t)){renderSession();return;}const s=store.sources.find(x=>x.key===sourceSelect.value);t.source=s;t.table=undefined;t.schema=undefined;t.detail=undefined;t.plan=undefined;t.page=undefined;t.executed='';t.view='data';store.selected=s;renderTab();if(s)void task(()=>openSource(s));};
editor.onscroll=scrollEditor;
editor.oninput=()=>{store.tab.sql=editor.value;renderText();completions.hidden=true;scheduleContext(350);};
formatButton.onclick=()=>{editor.value=formatSql(editor.value);editor.oninput?.(new Event('input'));};
history.onchange=()=>{if(history.value){editor.value=history.value;editor.oninput?.(new Event('input'));history.value='';editor.focus();}};
editor.onkeydown=e=>{
  if((e.ctrlKey||e.metaKey)&&e.key==='Enter'){e.preventDefault();if(!runButton.disabled)runButton.click();return;}
  if(e.key==='Escape'){completions.hidden=true;return;}
  if((e.ctrlKey||e.metaKey)&&e.code==='Space'){
    e.preventDefault();const pos=editor.selectionStart, prefix=/[\w\u0080-\uFFFF]+$/.exec(editor.value.slice(0,pos))?.[0]??'';const t=store.tab;
    const targets=suggestions(prefix,[...(t.source?.tables.values()??[])].flatMap(ts=>ts.map(r=>r.name)).concat(t.detail?.columns.map(c=>c.name)??t.page?.columns.map(c=>c.name)??[]));
    completions.replaceChildren(...targets.map(value=>{const b=btn(value);b.onclick=()=>{editor.setRangeText(value,pos-prefix.length,pos,'end');editor.oninput?.(new Event('input'));editor.focus();};return b;}));completions.hidden=!targets.length;completions.querySelector('button')?.focus();
  }
};
completions.onkeydown=e=>{if(e.key==='Escape'){completions.hidden=true;editor.focus();}};
function splitAt(height:number){work.style.setProperty('--editor-h',Math.round(Math.max(125,Math.min(work.clientHeight*.56,height)))+'px');}
split.onpointerdown=e=>{split.setPointerCapture(e.pointerId);const origin=e.clientY,h=editorPane.clientHeight;split.onpointermove=m=>splitAt(h+m.clientY-origin);split.onlostpointercapture=()=>{split.onpointermove=null;};};
split.onkeydown=e=>{if(e.key==='ArrowUp'||e.key==='ArrowDown'){e.preventDefault();splitAt(editorPane.clientHeight+(e.key==='ArrowDown'?18:-18));}};

function connectionDialog(prior?:Connection){
  const m=modal(prior?'编辑连接':'新建连接');
  const name=input('连接名称');const driver=select('数据库类型',[['sqlite','SQLite'],['mysql','MySQL / MariaDB'],['postgres','PostgreSQL']]);
  const env=select('环境',[['dev','DEV'],['staging','STAGING'],['prod','PROD']]);const ro=input('只读连接','checkbox');
  const path=input('SQLite 文件路径');path.placeholder='data/app.sqlite';const host=input('主机');const port=input('端口','number');port.min='1';port.max='65535';const database=input('数据库');const username=input('用户名');const password=input('密码','password');password.autocomplete='new-password';password.placeholder=prior?.has_password?'留空保留已保存密码':'';
  const ssl=select('TLS',[['preferred','优先 TLS'],['required','必须 TLS'],['verify_ca','验证 CA'],['verify_full','验证 CA 和主机'],['disabled','禁用 TLS']]);const notes=input('备注');
  name.value=prior?.name??'';driver.value=prior?.driver??'sqlite';env.value=prior?.env??'dev';ro.checked=prior?.read_only??false;path.value=prior?.file_path??'';host.value=prior?.host??'';port.value=prior?.port?String(prior.port):'';database.value=prior?.database??'';username.value=prior?.username??'';ssl.value=prior?.ssl_mode??'preferred';notes.value=prior?.notes??'';
  const localBox=el('div','db-form');localBox.append(field('SQLite 文件路径',path));const remote=el('div','db-form db-form-two');remote.append(field('主机',host),field('端口',port),field('数据库',database),field('用户名',username),field('密码',password),field('TLS',ssl));
  const layout=el('div','db-form');const pair=el('div','db-form db-form-two');pair.append(field('数据库类型',driver),field('环境',env));layout.append(field('连接名称',name),pair,localBox,remote,field('备注',notes));const checkbox=el('label','db-check');checkbox.append(ro,'只读连接');layout.append(checkbox);
  const feedback=el('p','db-dialog-feedback');feedback.setAttribute('role','status');m.body.append(layout,feedback);
  const update=()=>{localBox.hidden=driver.value!=='sqlite';remote.hidden=driver.value==='sqlite';port.placeholder=driver.value==='postgres'?'5432':'3306';};driver.onchange=update;update();
  const values=()=>({id:prior?.id,name:name.value.trim(),driver:driver.value as Kind,env:env.value as Env,read_only:ro.checked,notes:notes.value,...(driver.value==='sqlite'?{file_path:path.value.trim()}:{host:host.value.trim(),port:port.value?Number(port.value):undefined,database:database.value.trim(),username:username.value.trim(),password:password.value,ssl_mode:ssl.value})});
  const save=btn('保存连接','check',true,'db-primary'),test=btn('测试连接','flask-conical');
  const action=(b:HTMLButtonElement,fn:()=>Promise<void>)=>b.onclick=async()=>{save.disabled=test.disabled=true;feedback.textContent='处理中…';try{await fn();}catch(e){feedback.textContent=e instanceof Error?e.message:String(e);}finally{save.disabled=test.disabled=false;}};
  action(test,async()=>{const t=await api.test(values());feedback.textContent=t.ok?'连接成功 · '+t.version:t.error??'连接失败';});
  action(save,async()=>{const r=await api.save(values());for(const s of store.sources)if(s.connection?.id===r.connection.id){s.schemas=undefined;s.tables.clear();}await loadSources();for(const t of store.tabs)if(t.source?.connection?.id===r.connection.id){t.page=undefined;t.patches.clear();t.executed='';t.detail=undefined;t.plan=undefined;t.view='data';}renderTab();m.close();status('连接已保存');});
  if(prior){const del=btn('删除连接','x');action(del,async()=>{if(!confirm('删除连接档案？数据库文件不会被删除。'))return;await api.delete(prior.id);for(const t of store.tabs)if(t.source?.connection?.id===prior.id){t.source=undefined;t.page=undefined;t.executed='';t.patches.clear();t.detail=undefined;t.plan=undefined;t.view='data';}await loadSources();renderTab();m.close();});m.foot.append(del);}
  m.foot.append(test,save);m.open();name.focus();
}
function approval(source:Source,body:HTMLElement,apply:()=>Promise<void>){
  const m=modal('确认数据库写入');m.body.append(body);
  const yes=btn('确认提交','check',true,'db-primary');const no=btn('取消');no.onclick=m.close;
  if(source.connection?.env==='prod'){const check=input('确认生产环境写入','checkbox');const l=el('label','db-check db-danger');l.append(check,'确认写入 PROD 生产环境');m.body.append(l);yes.disabled=true;check.onchange=()=>yes.disabled=!check.checked;}
  const feedback=el('p','db-dialog-feedback');feedback.setAttribute('role','status');m.body.append(feedback);
  yes.onclick=async()=>{yes.disabled=true;no.disabled=true;try{await apply();m.close();}catch(e){feedback.textContent=e instanceof Error?e.message:String(e);yes.disabled=false;}finally{no.disabled=false;}};
  m.foot.append(no,yes);m.open();
}
bind(submit,async()=>{
  const t=store.tab,e=t.page?.editable_table,s=t.source;if(!e||!s||!t.patches.size)return;
  const data={schema:e.schema,table:e.table,patch:[...t.patches.values()]};const preview=await api.execute(s.target,{...data,preview:true});
  if(t!==store.tab)return;
  const block=el('div','db-sql-preview');for(const cmd of preview.statements??[])block.append(el('pre','',cmd.sql+'\n'+JSON.stringify(cmd.params)));
  approval(s,block,async()=>{const r=await api.execute(s.target,{...data,confirm_prod:s.connection?.env==='prod'});t.patches.clear();await run(t,t.page?.offset??0,t.executed);status('已提交 · '+r.rows_affected+' 行');});
});
discard.onclick=()=>{store.tab.patches.clear();grid.refresh();renderResult(false);status('已放弃修改');};
bind(writeButton,async()=>{
  const t=store.tab,s=t.source,sql=selectedSql();if(!s||!sql||!confirmDiscard(t))return;
  const r=await api.execute(s.target,{sql,preview:true});const text=(r.statements??[]).map(x=>x.sql).join('\n');
  approval(s,el('pre','db-value',text),async()=>{const result=await api.execute(s.target,{sql,confirm_prod:s.connection?.env==='prod'});t.page=undefined;t.executed='';t.plan=undefined;t.detail=undefined;t.view='data';s.tables.clear();if(s.schema)await openSchema(s,s.schema,false);if(t===store.tab)renderResult(true);status('写入完成 · '+result.rows_affected+' 行');});
});
bind(exportButton,()=>{
  const t=store.tab,s=t.source;if(!s||!t.executed)return;
  const m=modal('导出 Excel (.xlsx)');const path=input('工作区导出路径');path.value='query-result.xlsx';m.body.append(field('工作区导出路径',path));
  const save=btn('保存到工作区','folder-open'),saveFile=btn('下载 .xlsx','download',true,'db-primary');
  bind(saveFile,async()=>{const blob=await api.download(s.target,t.executed);download(blob,'query-result.xlsx');m.close();status('已导出 Excel');});
  bind(save,async()=>{const r=await api.exportPath(s.target,t.executed,path.value);m.close();status('已导出 '+r.rows+' 行 · '+r.path+(r.truncated?'（已截断）':''));});m.foot.append(save,saveFile);m.open();
});
importButton.onclick=()=>{
  const s=store.tab.source;if(!s)return;
  const m=modal('从 Excel 导入');const path=input('工作区 XLSX 路径');path.placeholder='data/input.xlsx';const upload=input('选择 XLSX 文件','file');upload.accept='.xlsx';const sheet=input('工作表名');sheet.placeholder='默认为第一张';const table=input('目标表名');const mode=select('导入方式',[['create_and_insert','建表并导入'],['append','追加到现有表']]);
  const form=el('div','db-form');form.append(field('工作区 XLSX 路径',path),field('或上传 XLSX',upload),field('工作表名',sheet),field('目标表名',table),field('导入方式',mode));const previewBox=el('div','db-import-preview');m.body.append(form,previewBox);
  const preview=btn('预览数据','table-2'),apply=btn('导入数据','check',true,'db-primary');apply.disabled=true;let cached:ImportInput|undefined;let busy=false;
  form.oninput=()=>{cached=undefined;apply.disabled=true;previewBox.replaceChildren();};
  bind(preview,async()=>{if(busy)return;busy=true;try{const file=upload.files?.[0];const picked=file?{xlsx_base64:await base64(file)}:{xlsx_path:path.value.trim()};const payload:ImportInput={...picked,sheet:sheet.value.trim()||undefined,schema:s.schema,table_name:table.value.trim(),mode:'preview'};const view=await api.import(s.target,payload);const summary=el('p','',(view.row_count??0)+' 行 · '+(view.columns??[]).map(c=>c.name+' '+c.type).join(' / '));previewBox.replaceChildren(summary,el('pre','',JSON.stringify(view.rows??[],null,2)));cached={...payload,mode:mode.value==='append'?'append':'create_and_insert'};apply.disabled=!payload.table_name;}finally{busy=false;}});
  bind(apply,()=>{if(!cached)return;const data=cached;m.close();approval(s,el('p','',(data.mode==='append'?'追加到表 ':'新建表并导入 ') +(data.schema?data.schema+'.':'')+data.table_name),async()=>{const r=await api.import(s.target,data,s.connection?.env==='prod');s.tables.clear();if(s.schema)await openSchema(s,s.schema,false);status('已导入 '+r.rows_inserted+' 行');});});
  m.foot.append(preview,apply);m.open();
};
function hostWorkspace():Promise<string>{
  if(api.workspace||!embed||parent===window)return Promise.resolve(api.workspace);
  return new Promise(resolve=>{
    const id=crypto.randomUUID();const timer=setTimeout(()=>done(''),1500);
    const receive=(e:MessageEvent)=>{if(e.source===parent&&e.origin===location.origin&&e.data?.type==='skill-package/response'&&e.data.id===id)done(typeof e.data.result?.workspace_root==='string'?e.data.result.workspace_root:'');};
    function done(root:string){clearTimeout(timer);removeEventListener('message',receive);resolve(root);}
    addEventListener('message',receive);parent.postMessage({type:'skill-package/request',id,method:'workspace'},location.origin);
  });
}
let fileGeneration=0;
async function openFile(raw:string,name?:string){
  const generation=++fileGeneration;let s:Source;
  if(!raw.startsWith('/agent/')&&!/^https?:/i.test(raw)){s=local(raw);}
  else {
    const u=new URL(raw,location.origin);if(u.origin!==location.origin||!u.pathname.startsWith('/agent/')||u.username||u.password)throw new Error('仅允许打开同源 QuickAgent 附件');
    const path=u.pathname.endsWith('/workspace-file')?u.searchParams.get('path'):null;
    if(path&&api.workspace)s=local(path);
    else{const r=await fetch(u,{redirect:'error',signal:AbortSignal.timeout(30_000)});if(!r.ok)throw new Error('无法读取附件');const id=await api.attachment(await r.blob());s={key:'attachment:'+id,name:name?.trim()||'SQLite 附件',target:{attachment_id:id},readOnly:true,schema:'main',tables:new Map()};}
  }
  if(generation!==fileGeneration)return;s=addSource(s);await openSource(s);if(generation!==fileGeneration)return;
  const found=s.schema?s.tables.get(s.schema)?.[0]:undefined;if(found)await openTable(s,s.schema!,found,false);
  if(params.get('mode')==='preview'&&parent!==window)parent.postMessage({type:'quickagent:preview-ready',kind:'db'},location.origin);
}
const boot=hostWorkspace().then(root=>{api.workspace=root;renderSession();});
function safeOpen(file:string,name?:string){return task(async()=>{await boot;try{await openFile(file,name);}catch(e){if(params.get('mode')==='preview'&&parent!==window)parent.postMessage({type:'quickagent:preview-error',message:e instanceof Error?e.message:String(e)},location.origin);throw e;}});}
let lastQuietRefresh=0;
function quietRefreshSources(){
  if(params.get('mode')==='preview')return;
  const now=Date.now();
  if(now-lastQuietRefresh<2500)return;
  lastQuietRefresh=now;
  void loadSources(true).catch(()=>{});
}
addEventListener('focus',quietRefreshSources);
document.addEventListener('visibilitychange',()=>{if(document.visibilityState==='visible')quietRefreshSources();});
if(params.get('mode')!=='preview')setInterval(()=>{if(document.visibilityState==='visible'&&!dbQueue)quietRefreshSources();},4000);
interface AiDbSyncPayload{action:'save_connection'|'inspect'|'query'|'explain'|'execute'|'focus_view';connectionId?:string;sqlitePath?:string;schema?:string;table?:string;sql?:string;column?:string;row?:number;range?:string;findColumn?:string;findValue?:string;}
function extractSqlTable(sql:string):{schema?:string;table:string}|undefined{
  const match=sql.match(/\bFROM\s+(?:[`"']?([\w-]+)[`"']?\.)?[`"']?([\w-]+)[`"']?/i);
  if(!match||!match[2])return undefined;
  const schema=match[1],table=match[2];
  const lowerSchema=(schema??'').toLowerCase(),lowerTable=table.toLowerCase();
  if(lowerSchema==='information_schema'||lowerSchema==='performance_schema'||lowerSchema==='pg_catalog'||lowerTable==='sqlite_master'||lowerTable==='dual')return undefined;
  return schema?{schema,table}:{table};
}
let aiSyncChain:Promise<void>=Promise.resolve();
async function resolveSyncSource(detail:AiDbSyncPayload):Promise<Source|undefined>{
  const find=():Source|undefined=>{
    if(detail.connectionId)return store.sources.find(s=>s.key==='conn:'+detail.connectionId);
    if(detail.sqlitePath)return store.sources.find(s=>s.key==='file:'+detail.sqlitePath);
    return store.tab.source??store.selected;
  };
  let found=find();
  if(!found||detail.action==='save_connection'){
    await loadSources(true);
    found=find();
  }
  return found;
}
function enqueueAiDbSync(detail:AiDbSyncPayload){
  if(params.get('mode')==='preview')return;
  aiSyncChain=aiSyncChain.then(async()=>{
    await boot;
    const src=await resolveSyncSource(detail);
    if(!src)return;
    if(detail.action==='save_connection'){
      await openSource(src,false);
      status(`已同步 AI 连接 · ${src.name}`);
      return;
    }
    if(detail.action==='inspect'){
      await openSource(src,false);
      const schema=detail.schema??src.schema;
      if(schema){
        await openSchema(src,schema,false);
        if(detail.table){
          const found=src.tables.get(schema)?.find(t=>t.name===detail.table)??{name:detail.table,kind:'table'};
          await openTable(src,schema,found,true);
          status(`已同步 AI 表结构 · ${detail.table}`);
        }
      }
      return;
    }
    if(detail.action==='query'){
      await openSource(src,false);
      const sql=(detail.sql??'').trim();
      const parsed=sql?extractSqlTable(sql):undefined;
      const schema=detail.schema??parsed?.schema??src.schema;
      const table=detail.table??parsed?.table;
      if(!table&&(!sql||!/\bFROM\b/i.test(sql)))return;
      if(schema&&table){
        await openSchema(src,schema,false);
        const existing=store.tabs.find(t=>t.source?.key===src.key&&t.schema===schema&&t.table===table);
        const tab=existing??store.open({label:table,source:src,schema,table,sql:sql||('SELECT * FROM '+quote(src.connection?.driver??'sqlite',schema)+'.'+quote(src.connection?.driver??'sqlite',table)),view:'data'});
        store.active=tab.id;tab.view='data';if(sql)tab.sql=sql;
        renderTab();await run(tab,0,tab.sql);
        status(`已同步 AI 打开表 · ${table}`);
      }else if(sql){
        const reuse=!store.tab.executed&&!store.tab.table&&!store.tab.patches.size;
        const tab=reuse?store.tab:store.open({label:table??'AI 查询',source:src,sql,view:'data'});
        tab.source=src;tab.sql=sql;tab.view='data';store.active=tab.id;
        renderTab();await run(tab,0,sql);
        status('已同步 AI 查询视图');
      }
      return;
    }
    if(detail.action==='explain'&&detail.sql){
      await openSource(src,false);
      const reuse=!store.tab.executed&&!store.tab.table&&!store.tab.patches.size;
      const tab=reuse?store.tab:store.open({label:'执行计划',source:src,sql:detail.sql,view:'plan'});
      tab.source=src;tab.sql=detail.sql;tab.plan=await api.explain(src.target,detail.sql);tab.view='plan';store.active=tab.id;
      renderTab();status('已同步 AI 执行计划');
      return;
    }
    if(detail.action==='execute'){
      src.tables.clear();
      if(src.schema)await openSchema(src,src.schema,false);
      const t=store.tab;
      if(t.source?.key===src.key&&t.executed&&!t.patches.size)await run(t,t.page?.offset??0,t.executed);
      status('已同步 AI 写入变更');
      return;
    }
    if(detail.action==='focus_view'){
      const targetTable=detail.table?.trim();
      if(targetTable){
        await openSource(src,false);
        const schema=detail.schema??src.schema;
        if(schema){
          await openSchema(src,schema,false);
          const existing=store.tabs.find(t=>t.source?.key===src.key&&t.table?.toLowerCase()===targetTable.toLowerCase());
          if(existing){
            store.active=existing.id;existing.view='data';renderTab();
            if(!existing.page&&existing.sql)await run(existing,0,existing.sql);
          }else{
            const found=src.tables.get(schema)?.find(t=>t.name.toLowerCase()===targetTable.toLowerCase())??{name:targetTable,kind:'table'};
            await openTable(src,schema,found,false);
          }
        }
      }
      if(store.tab.view!=='data'){store.tab.view='data';renderResult(false);}
      const picked=grid.selectTarget({column:detail.column,row:detail.row,range:detail.range,findColumn:detail.findColumn,findValue:detail.findValue});
      if(picked)status(`已定位到 ${picked}`);
    }
  }).catch(()=>{});
}
addEventListener('message',e=>{if(parent===window||e.source!==parent||e.origin!==location.origin)return;if(e.data?.type==='quickagent:theme')setTheme(e.data.theme);if(e.data?.type==='quickagent:refresh-sources')quietRefreshSources();if(e.data?.type==='quickagent:db-sync'&&e.data.detail&&typeof e.data.detail==='object')enqueueAiDbSync(e.data.detail as AiDbSyncPayload);if(['quickagent:open-file','quickagent:reload-file'].includes(e.data?.type)&&typeof e.data.file==='string')void safeOpen(e.data.file,e.data.name);});
if(embed&&parent!==window){const root=parent.document.documentElement;new MutationObserver(()=>setTheme(root.hasAttribute('data-ds-dark-theme')?'dark':'light')).observe(root,{attributes:true,attributeFilter:['data-ds-dark-theme']});}
let contextTimer:number|undefined;
function buildStudioContext(){
  const t=store.tab,s=t.source;
  if(!s&&!t.page)return null;
  const lines:string[]=[];
  if(s){
    const targetParts:string[]=[`连接名称: ${s.name}`];
    if('connection_id' in s.target&&s.target.connection_id)targetParts.push(`connection_id: ${s.target.connection_id}`);
    if('sqlite_path' in s.target&&s.target.sqlite_path)targetParts.push(`sqlite_path: ${s.target.sqlite_path}`);
    if(s.connection?.driver)targetParts.push(`driver: ${s.connection.driver}`);
    if(s.connection?.env)targetParts.push(`env: ${s.connection.env}`);
    if(s.connection?.database)targetParts.push(`database: ${s.connection.database}`);
    targetParts.push(`只读: ${s.readOnly?'是':'否'}`);
    lines.push(targetParts.join(' | '));
  }
  const tableRef=t.table?(t.schema?`${t.schema}.${t.table}`:t.table):undefined;
  if(tableRef)lines.push(`当前表: ${tableRef}（视图: ${t.view}）`);
  const sql=(t.executed||t.sql).trim();
  if(sql)lines.push(`当前 SQL:\n\`\`\`sql\n${sql.slice(0,1200)}\n\`\`\``);
  let detail='已连接';
  if(t.page){
    const snap=grid.snapshot(15,160);
    lines.push(`结果列 (${snap.columns.length}): ${snap.columns.join(', ')}`);
    const rangeText=t.page.rows.length?`${t.page.offset+1}–${t.page.offset+t.page.rows.length}${t.page.truncated?'+':''}`:'0';
    lines.push(`当前页行数: ${snap.totalVisible}（分页区间 ${rangeText}${filterGrid.value?`，筛选词 "${filterGrid.value}"`:''}）`);
    if(snap.selectedRows.length){
      detail=snap.selectedRowCount>1?`已选 ${snap.selectedRowCount} 行 / 共 ${snap.totalVisible} 行`:`已定位第 1 条选中行 / 共 ${snap.totalVisible} 行`;
      lines.push(`用户选中的数据行 (${snap.selectedRowCount} 行，展示前 ${snap.selectedRows.length} 行):\n\`\`\`json\n${JSON.stringify(snap.selectedRows,null,2)}\n\`\`\``);
    }else{
      detail=`共 ${snap.totalVisible} 行`;
    }
    if(snap.sampleRows.length){
      lines.push(`当前页前 ${snap.sampleRows.length} 行样例:\n\`\`\`json\n${JSON.stringify(snap.sampleRows,null,2)}\n\`\`\``);
    }
  }else if(t.view==='structure'&&t.detail){
    detail=`表结构 · ${t.detail.columns.length} 列`;
    lines.push(`表结构 (${t.detail.schema}.${t.detail.table}):\n`+t.detail.columns.map(c=>`- ${c.name}: ${c.type}${c.primary_key?' [PK]':''}${c.nullable?'':' NOT NULL'}`).join('\n'));
  }else if(sql){
    detail='SQL 草稿';
  }
  let text=lines.join('\n');
  if(text.length>4200)text=text.slice(0,4200)+'\n…(视图上下文已截断，如需更多行请直接调用 db.query 查询)';
  const badge=`数据库 · ${tableRef??s?.name??'当前视图'}`;
  const id=`db:${s?.key??''}:${tableRef??''}:${t.view}:${sql.slice(0,80)}:${detail}`;
  return {plugin:'db',id,badge,detail,text};
}
function emitStudioContext(){
  if(parent===window||params.get('mode')==='preview')return;
  const ctx=buildStudioContext();
  parent.postMessage({type:'quickagent:studio-context',plugin:'db',context:ctx},location.origin);
}
function scheduleContext(delay=150){
  if(contextTimer!==undefined)clearTimeout(contextTimer);
  contextTimer=window.setTimeout(()=>{contextTimer=undefined;emitStudioContext();},delay);
}
askAiButton.onclick=()=>{
  emitStudioContext();
  if(parent!==window)parent.postMessage({type:'quickagent:ask-ai',plugin:'db'},location.origin);
};

renderTab();void task(async()=>{await boot;await loadSources();const f=params.get('file');if(f)await safeOpen(f,params.get('name')??undefined);});

