import { connectStudio, fetchStudioFile, preview } from './quickagent-preview';
import './styles/studio.css';
import {ApiClient} from './api-client';
import {button,element as el,icon,menu,segment} from './components/controls';
import {Grid} from './grid';
import {blankStyle,fromA1,rangeA1,rangeSize,toA1,type CellStyle,type Coord,type DimensionChange,type Workbook} from './model';
import {WorkbookStore,aggregate} from './store';

const api=new ApiClient();
const store=new WorkbookStore(api);
const app=document.querySelector<HTMLDivElement>('#app')!;
app.className='studio';
let draft: {coord:Coord;sheet:string;text:string;original:string}|undefined;
let requestId=0;
let loading=false;
let toastTimer=0;
let results: Array<{sheet:number;address:string}>=[];
let resultIndex=-1;

const header=el('header','topbar');
const brand=el('div','brand');
const logo=el('div','brand-icon');logo.append(icon('sheet'));
const brandName=el('h1','brand-name','Excel 表格');
const fileName=el('span','file-name',store.title);
const badge=el('span','file-badge','XLSX');
const separator=el('span','crumb');separator.append(icon('chevron-right'));
brand.append(logo,brandName,separator,fileName,badge);
const fileActions=el('div','file-actions');
const sample=button('载入示例工作簿','flask-conical','示例','file-button sample-button');
const fresh=button('新建工作簿','file-plus-2','新建','file-button');
const open=button('打开 .xlsx','folder-open','打开','file-button');
const save=button('导出 .xlsx','download','导出 XLSX','file-button primary');
const theme=button('切换主题','moon',undefined,'file-button theme-button');
const file=el('input','file-input');file.type='file';file.accept='.xlsx,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet';file.hidden=true;file.id='xlsx-file';
fileActions.append(sample,fresh,open,save,theme,file);header.append(brand,fileActions);

const toolbar=el('div','toolbar');toolbar.setAttribute('role','toolbar');toolbar.setAttribute('aria-label','工作表工具');
const undo=button('撤销 (Ctrl+Z)','undo-2');undo.dataset.action='undo';
const redo=button('重做 (Ctrl+Y)','redo-2');redo.dataset.action='redo';
const bold=button('加粗 (Ctrl+B)','bold'), italic=button('斜体 (Ctrl+I)','italic');
const left=button('左对齐','align-left'), center=button('居中','align-center'), right=button('右对齐','align-right');
const styleButtons=[bold,italic,left,center,right];
const merge=button('合并单元格','merge'), split=button('取消合并','split');
const find=button('查找 (Ctrl+F)','search','查找','tool-button find-button');
const numberFormats=[['常规',null],['货币 (¥)','¥#,##0.00'],['百分比','0.0%'],['千分位','#,##0.00'],['整数','0']] as const;
const number=el('select','number-format');number.title='数字格式';number.setAttribute('aria-label','数字格式');
for(const [label,value] of numberFormats) {const o=el('option','',label);o.value=value??'';number.append(o);}
const custom=el('option','','自定义格式');custom.value='custom';custom.hidden=true;number.append(custom);
const fmt=el('label','format-select');fmt.append(icon('calculator'),number,icon('chevron-down'));
const percentage=button('百分比格式','percent');
const dimensions=menu('编辑行列','table-2',[
  {label:'在上方插入一行',icon:'rows-3',onClick:()=>dimensionsOp({action:'insert_rows',at:store.range.start.row})},
  {label:'删除选区所在行',icon:'rows-3',onClick:()=>dimensionsOp({action:'delete_rows',at:store.range.start.row,count:store.range.end.row-store.range.start.row+1})},
  {label:'在左侧插入一列',icon:'columns-3',onClick:()=>dimensionsOp({action:'insert_columns',at:store.range.start.col})},
  {label:'删除选区所在列',icon:'columns-3',onClick:()=>dimensionsOp({action:'delete_columns',at:store.range.start.col,count:store.range.end.col-store.range.start.col+1})},
  {label:'冻结或取消冻结首行',icon:'panel-top',onClick:()=>dimensionsOp({action:'freeze_first_row',frozen:!store.sheet.frozen})},
]);
const palette=['#172620','#FFFFFF','#137351','#DAEFE5','#B64C33','#F7E3D6','#1857A3','#DCEBFF','#68489C','#F2E7FF','#FBE8A6','#E4E9E7'];
function colorMenu(label:string,glyph:'baseline'|'paint-bucket',key:'color'|'fill') {
  const ctrl=menu(label,glyph,[]);ctrl.list.classList.add('palette');
  ctrl.list.append(el('div','palette-title',label));
  const shades=el('div','swatches');
  for(const color of palette) {
    const swatch=button(color,undefined,undefined,'swatch');swatch.style.setProperty('--swatch',color);
    swatch.addEventListener('click',()=>{ctrl.root.open=false;applyStyle({[key]:color});});shades.append(swatch);
  }
  const customColor=el('input','color-picker');customColor.type='color';customColor.value=key==='fill'?'#daefe5':'#137351';
  customColor.setAttribute('aria-label','自定义'+label);customColor.addEventListener('change',()=>{ctrl.root.open=false;applyStyle({[key]:customColor.value});});
  const reset=button('清除'+label,undefined,'清除颜色','menu-item');reset.addEventListener('click',()=>{ctrl.root.open=false;applyStyle({[key]:null});});
  const footer=el('div','palette-footer');footer.append(customColor,reset);ctrl.list.append(shades,footer);return ctrl.root;
}
const fontColor=colorMenu('字体颜色','baseline','color');
const fill=colorMenu('填充颜色','paint-bucket','fill');
toolbar.append(segment(undo,redo),segment(bold,italic),segment(left,center,right),segment(fontColor,fill),segment(fmt,percentage),segment(merge,split,dimensions.root),segment(find));

const formulaBar=el('div','formula-bar');
const address=el('input','address');address.setAttribute('aria-label','名称框');address.id='name-input';address.spellcheck=false;
const fxMark=el('label','fx-mark','fx');fxMark.htmlFor='fx-input';fxMark.title='公式栏';
const fx=el('input','fx-input');fx.id='fx-input';fx.spellcheck=false;fx.setAttribute('aria-label','公式栏');fx.autocomplete='off';
const accept=button('应用编辑','check',undefined,'formula-action');accept.hidden=true;
const discard=button('取消编辑','x',undefined,'formula-action');discard.hidden=true;
formulaBar.append(address,fxMark,fx,discard,accept);

const sheetBody=el('main','sheet-body');sheetBody.setAttribute('aria-label','工作表');
const grid=new Grid(store,{start:startInline,draft:setDraft,commit,cancel,error:showError},preview);
const drop=el('div','drop-overlay');const dropLabel=el('div','drop-label');dropLabel.append(icon('folder-open'),el('span','','导入 .xlsx'));drop.append(dropLabel);drop.hidden=true;
const searchPanel=el('form','search-panel');searchPanel.setAttribute('role','search');searchPanel.hidden=true;
const searchTerm=el('input','search-term');searchTerm.placeholder='查找值或公式';searchTerm.setAttribute('aria-label','查找值或公式');
const searchCount=el('output','search-count','0 / 0');
const prev=button('上一处','arrow-up'), next=button('下一处','arrow-down'), close=button('关闭查找','x');
searchPanel.append(icon('search'),searchTerm,searchCount,prev,next,close);
sheetBody.append(grid.root,searchPanel,drop);

const sheetbar=el('div','sheetbar');
const tabs=el('div','sheet-tabs');tabs.setAttribute('role','tablist');tabs.setAttribute('aria-label','工作表标签');
const newSheet=button('添加工作表','plus');
const sheetMenu=menu('管理工作表','ellipsis',[
  {label:'重命名工作表',onClick:()=>void renameSheet()},
  {label:'复制工作表',onClick:()=>void copySheet()},
  {label:'删除工作表',onClick:()=>void deleteSheet()},
],true);
sheetbar.append(tabs,newSheet,sheetMenu.root);

const statusbar=el('footer','statusbar');
const status=el('div','engine-status');status.setAttribute('role','status');
const light=el('span','status-dot');const statusText=el('span','','就绪');status.append(light,statusText);
const rangeLabel=el('span','range-label');
const statistics=el('div','statistics');statistics.setAttribute('aria-label','选区统计');
const items=[['count','计数'],['sum','求和'],['average','均值'],['min','最小'],['max','最大']] as const;
const statNodes=Object.fromEntries(items.map(([id,label])=>{
  const node=el('div','stat');const val=el('strong','stat-value','—');val.dataset.stat=id;
  node.append(el('span','stat-label',label),val);statistics.append(node);return [id,val];
}));
statusbar.append(status,rangeLabel,statistics);
const toast=el('aside','toast');toast.hidden=true;toast.setAttribute('role','alert');
const toastText=el('span','toast-message'); const closeToast=button('关闭提示','x');closeToast.addEventListener('click',()=>{toast.hidden=true;});toast.append(icon('triangle-alert'),toastText,closeToast);

const dialog=el('dialog','dialog');const dialogForm=el('form','dialog-form');dialogForm.method='dialog';
const dialogTitle=el('h2','dialog-title');dialogTitle.id='dialog-title';dialog.setAttribute('aria-labelledby',dialogTitle.id);
const dialogName=el('input','dialog-input');dialogName.maxLength=31;dialogName.setAttribute('aria-label','工作表名称');
const dialogMessage=el('p','dialog-message');
const dialogActions=el('div','dialog-actions');const dialogCancel=button('取消',undefined,'取消','file-button');dialogCancel.addEventListener('click',()=>dialog.close('cancel'));
const dialogSubmit=el('button','file-button primary','确定');dialogSubmit.type='submit';dialogSubmit.value='confirm';
dialogActions.append(dialogCancel,dialogSubmit);dialogForm.append(dialogTitle,dialogMessage,dialogName,dialogActions);dialog.append(dialogForm);
app.append(header,toolbar,formulaBar,sheetBody,sheetbar,statusbar,toast,dialog);

function editUI() {accept.hidden=!draft;discard.hidden=!draft;fx.classList.toggle('editing',!!draft);}
function begin(at:Coord,text?:string) {
  if(preview)return;
  const original=store.sheet.cells[toA1(at)]?.raw??'';
  draft={coord:at,sheet:store.sheet.name,text:text??original,original};
  fx.value=draft.text;editUI();
}
function startInline(at:Coord,text?:string) {if(preview)return;commit();begin(at,text);grid.openEditor(at,draft!.text);}
function setDraft(text:string) {if(preview)return;if(!draft)begin(store.active);draft!.text=text;if(fx.value!==text)fx.value=text;grid.updateEditor(text);}
function commit(move?:{row:number;col:number}) {
  if(draft) {
    const edit=draft;draft=undefined;grid.closeEditor();editUI();
    if(edit.text!==edit.original)void store.write({[toA1(edit.coord)]:edit.text},edit.sheet).catch(showError);
    else fx.value=store.cell?.raw??'';
  }
  if(move)grid.move(move);
}
function cancel() {draft=undefined;grid.closeEditor();fx.value=store.cell?.raw??'';editUI();}
fx.addEventListener('focus',()=>{if(!draft)begin(store.active);});fx.addEventListener('input',()=>setDraft(fx.value));
fx.addEventListener('keydown',e=>{
  if(e.isComposing)return;
  if(e.key==='Escape'){e.preventDefault();cancel();grid.focus();}
  if(e.key==='Enter'||e.key==='Tab'){e.preventDefault();commit(e.key==='Enter'?{row:e.shiftKey?-1:1,col:0}:{row:0,col:e.shiftKey?-1:1});grid.focus();}
});
fx.addEventListener('blur',e=>{if(e.relatedTarget!==accept&&e.relatedTarget!==discard)commit();});
accept.addEventListener('click',()=>{commit();grid.focus();});discard.addEventListener('click',()=>{cancel();grid.focus();});
address.addEventListener('keydown',e=>{
  if(e.key==='Enter'){e.preventDefault();if(store.jump(address.value)){grid.reveal(store.active);grid.focus();}else showError(new Error('请输入有效的 A1 地址或区域'));}
  else if(e.key==='Escape'){address.value=rangeA1(store.range);grid.focus();}
});
address.addEventListener('focus',()=>address.select());address.addEventListener('blur',()=>{address.value=rangeA1(store.range);});

const doUndo=()=>{cancel();store.undo();grid.reveal(store.active);grid.focus();};
const doRedo=()=>{cancel();store.redo();grid.reveal(store.active);grid.focus();};
undo.addEventListener('click',doUndo);redo.addEventListener('click',doRedo);
function applyStyle(patch:Partial<CellStyle>) {commit();void store.format(patch).catch(showError);grid.focus();}
function dimensionsOp(change:DimensionChange){commit();void store.dimensions(change).catch(showError);grid.focus();}
const toggleBold=()=>applyStyle({bold:!store.cell?.style.bold});const toggleItalic=()=>applyStyle({italic:!store.cell?.style.italic});
bold.addEventListener('click',toggleBold);italic.addEventListener('click',toggleItalic);
for(const [btn,align] of [[left,'left'],[center,'center'],[right,'right']] as const)btn.addEventListener('click',()=>applyStyle({align}));
number.addEventListener('change',()=>{if(number.value!=='custom')applyStyle({numberFormat:number.value||null});});
percentage.addEventListener('click',()=>applyStyle({numberFormat: '0.0%'}));
for(const [btn,value] of [[merge,true],[split,false]] as const)btn.addEventListener('click',()=>{commit();void store.format(undefined,value).catch(showError);grid.focus();});

function showError(error:unknown) {
  toastText.textContent=error instanceof Error ? error.message : String(error);toast.hidden=false;
  clearTimeout(toastTimer);toastTimer=window.setTimeout(()=>{toast.hidden=true;},7000);
}
function refreshBusy() {
  const busy=store.busy||loading;
  statusText.textContent=loading?'正在打开':store.busy?'正在计算':'就绪';status.classList.toggle('working',busy);
  undo.disabled=!store.canUndo;redo.disabled=!store.canRedo;save.disabled=busy;
}
const numberDisplay=new Intl.NumberFormat('zh-CN',{maximumFractionDigits:3});
function refreshSelection() {
  const r=store.range;const formatted=rangeA1(r);
  if(document.activeElement!==address)address.value=formatted;
  if(!draft)fx.value=store.cell?.raw??'';
  rangeLabel.textContent=formatted;rangeLabel.title=formatted;
  const st=store.cell?.style??blankStyle();
  styleButtons.forEach((b,i)=>b.setAttribute('aria-pressed',String([st.bold,st.italic,st.align==='left',st.align==='center',st.align==='right'][i])));
  number.value=numberFormats.some(f=>f[1]===st.numberFormat) ? st.numberFormat??'' : 'custom';
  merge.disabled=rangeSize(r)<2;
  const stats=aggregate(store.sheet,r);
  for(const [name] of items){const value=stats[name];statNodes[name].textContent=value===null||(name==='sum'&&!stats.numericCount)?'—':numberDisplay.format(value);}
}
function refreshTabs() {
  const selected=document.activeElement instanceof HTMLElement ? document.activeElement.dataset.sheetTab : undefined;
  tabs.replaceChildren(...store.workbook.sheets.map((s,i)=>{
    const tab=button(s.name,undefined,s.name,'sheet-tab');tab.dataset.sheetTab=String(i);tab.setAttribute('role','tab');tab.setAttribute('aria-selected',String(i===store.index));tab.tabIndex=i===store.index?0:-1;
    tab.addEventListener('click',()=>{commit();store.switchSheet(i);grid.focus();});
    if(!preview)tab.addEventListener('dblclick',()=>void renameSheet());
    tab.addEventListener('keydown',e=>{if(e.key==='ArrowLeft'||e.key==='ArrowRight'){e.preventDefault();commit();store.switchSheet((i+store.workbook.sheets.length+(e.key==='ArrowLeft'?-1:1))%store.workbook.sheets.length);tabs.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus();}});
    return tab;
  }));
  const active=tabs.querySelector<HTMLButtonElement>('[aria-selected="true"]');active?.scrollIntoView({block:'nearest',inline:'nearest'});
  if(selected!==undefined)active?.focus({preventScroll:true});
  fileName.textContent=store.title;fileName.title=store.title;
}
store.subscribe(change=>{refreshBusy();if(change==='busy')return;if(change==='load'||change==='sheet'||change==='data')refreshTabs();refreshSelection();if(!searchPanel.hidden&&change==='data')findAll();});

async function load(loader:()=>Promise<Workbook>,title:string) {
  const id=++requestId;commit();loading=true;refreshBusy();
  try {await store.idle();const w=await loader();if(id===requestId){cancel();store.load(w,title);closeSearch(false);grid.focus();}}
  catch(error){if(id===requestId)showError(error);}
  finally {if(id===requestId){loading=false;refreshBusy();}}
}
const loadSample=()=>load(()=>api.sample(),'2026 Q1 营收分析');
sample.addEventListener('click',()=>void loadSample());fresh.addEventListener('click',()=>void load(()=>api.create(),'未命名工作簿'));
open.addEventListener('click',()=>file.click());
function loadFile(selected:File){
  if(preview)return;
  if(!/\.xlsx$/i.test(selected.name)){showError(new Error('请选择 .xlsx 文件'));return;}
  if(selected.size>32*1024*1024){showError(new Error('文件不能超过 32 MiB'));return;}
  return load(()=>api.parse(selected),selected.name.replace(/\.xlsx$/i,''));
}
file.addEventListener('change',()=>{const selected=file.files?.[0];file.value='';if(selected)void loadFile(selected);});
save.addEventListener('click',async()=>{
  commit();try {await store.idle();const blob=await api.export(store.workbook);const url=URL.createObjectURL(blob);
    const a=el('a');a.href=url;a.download=store.title.replace(/[\\/:*?"<>|]/g,'_')+'.xlsx';document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),30000);
  }catch(error){showError(error);}
});
let dragDepth=0;
app.addEventListener('dragenter',e=>{if(e.dataTransfer?.types.includes('Files')){e.preventDefault();dragDepth++;drop.hidden=false;}});
app.addEventListener('dragover',e=>{if(e.dataTransfer?.types.includes('Files')){e.preventDefault();e.dataTransfer.dropEffect='copy';}});
app.addEventListener('dragleave',()=>{if(--dragDepth<=0){dragDepth=0;drop.hidden=true;}});
app.addEventListener('drop',e=>{if(!e.dataTransfer?.files.length)return;e.preventDefault();drop.hidden=true;dragDepth=0;void loadFile(e.dataTransfer.files[0]);});

function updateTheme() {
  const isDark=document.documentElement.dataset.theme==='dark';theme.replaceChildren(icon(isDark?'sun':'moon'));
  theme.title=isDark?'切换为浅色':'切换为深色';theme.setAttribute('aria-label',theme.title);
}
theme.addEventListener('click',()=>{
  const next=document.documentElement.dataset.theme==='dark'?'light':'dark';document.documentElement.dataset.theme=next;
  try{localStorage.setItem('excel-studio-theme',next);}catch{ /* Embedded storage can be disabled. */ }updateTheme();
});
function findAll() {
  const q=searchTerm.value.trim().toLowerCase();results=[];resultIndex=-1;
  if(q)store.workbook.sheets.forEach((s,sheet)=>{for(const [address,c] of Object.entries(s.cells))if(c.display.toLowerCase().includes(q)||c.formula?.toLowerCase().includes(q))results.push({sheet,address});});
  searchCount.value='0 / '+results.length;
}
function navigateResult(delta:number) {
  if(!results.length)return;
  resultIndex=resultIndex<0?(delta>0?0:results.length-1):(resultIndex+delta+results.length)%results.length;
  const hit=results[resultIndex];store.switchSheet(hit.sheet);const p=fromA1(hit.address)!;store.locate(p);grid.reveal(p);
  searchCount.value=(resultIndex+1)+' / '+results.length;
}
function showSearch(){commit();searchPanel.hidden=false;searchTerm.focus();searchTerm.select();}
function closeSearch(focus=true){searchPanel.hidden=true;if(focus)grid.focus();}
find.addEventListener('click',showSearch);searchTerm.addEventListener('input',findAll);searchPanel.addEventListener('submit',e=>{e.preventDefault();navigateResult(1);});
searchPanel.addEventListener('keydown',e=>{if(e.key==='Escape'){e.stopPropagation();closeSearch();}else if(e.key==='Enter'){e.preventDefault();navigateResult(e.shiftKey?-1:1);}});
prev.addEventListener('click',()=>navigateResult(-1));next.addEventListener('click',()=>navigateResult(1));close.addEventListener('click',()=>closeSearch());

function ask(title:string,value?:string, message=''):Promise<string|null> {
  if(dialog.open)return Promise.resolve(null);
  dialogTitle.textContent=title;dialogName.hidden=value===undefined;dialogName.required=value!==undefined;dialogName.value=value??'';
  dialogMessage.hidden=!message;dialogMessage.textContent=message;dialog.returnValue='cancel';dialog.showModal();
  if(value!==undefined){dialogName.focus();dialogName.select();}else dialogCancel.focus();
  return new Promise(resolve=>dialog.addEventListener('close',()=>resolve(dialog.returnValue==='confirm'?(value===undefined?'yes':dialogName.value.trim()):null),{once:true}));
}
function nextSheetName(){let i=1;while(store.workbook.sheets.some(s=>s.name.toLowerCase()===('sheet'+i)))i++;return 'Sheet'+i;}
newSheet.addEventListener('click',async()=>{
  commit();const name=await ask('添加工作表',nextSheetName());if(name)void store.manage({action:'new',name}).catch(showError);grid.focus();
});
async function renameSheet(){const sheet=store.sheet.name;commit();const name=await ask('重命名工作表',sheet);if(name&&name!==sheet)void store.manage({action:'rename',sheet,name}).catch(showError);grid.focus();}
async function copySheet(){const sheet=store.sheet.name;commit();const name=await ask('复制工作表',sheet.slice(0,24)+' 副本');if(name)void store.manage({action:'copy',sheet,name}).catch(showError);grid.focus();}
async function deleteSheet(){
  if(store.workbook.sheets.length<=1){showError(new Error('至少保留一张工作表'));return;}
  const sheet=store.sheet.name;commit();if(await ask('删除工作表',undefined,'确定删除「'+sheet+'」？'))void store.manage({action:'delete',sheet}).catch(showError);grid.focus();
}
document.addEventListener('pointerdown',e=>{if(!(e.target instanceof Element))return;document.querySelectorAll<HTMLDetailsElement>('details.menu[open]').forEach(menu=>{if(!menu.contains(e.target as Node))menu.open=false;});});
document.addEventListener('keydown',e=>{
  if(!(e.ctrlKey||e.metaKey)||e.isComposing||dialog.open)return;
  const key=e.key.toLowerCase();
  if(key==='f'){e.preventDefault();showSearch();return;}
  if(preview)return;
  if(key==='s'){e.preventDefault();if(!save.disabled)save.click();return;}
  if(e.target instanceof HTMLInputElement)return;
  if(key==='z'){e.preventDefault();if(e.shiftKey)doRedo();else doUndo();}
  else if(key==='y'){e.preventDefault();doRedo();}
  else if(key==='b'){e.preventDefault();toggleBold();}
  else if(key==='i'){e.preventDefault();toggleItalic();}
});
if(preview){
  for(const node of [sample,fresh,open,save,toolbar,newSheet,sheetMenu.root])node.style.display='none';
  fx.readOnly=true;
}
updateTheme();refreshBusy();refreshTabs();refreshSelection();
const hosted=connectStudio({kind:'excel',theme(value){document.documentElement.dataset.theme=value;updateTheme();},error:showError,async load(url,name,signal,reload){
  const selected=await fetchStudioFile(url,name,signal);
  if(!/\.xlsx$/i.test(name)||selected.size>32*1024*1024)throw new Error('仅支持 32 MiB 以内的 .xlsx 文件');
  const w=await api.parse(selected);
  if(signal.aborted)return;
  const sheet=reload?store.sheet.name:undefined;
  cancel();store.load(w,name.replace(/\.xlsx$/i,''));
  const index=w.sheets.findIndex(item=>item.name===sheet);
  if(index>0)store.switchSheet(index);
  return {sheets:w.sheets.length};
}});
if(!hosted)void loadSample();
