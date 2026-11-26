import {Grid} from '../../excel-studio/src/grid';
import {WorkbookStore} from '../../excel-studio/src/store';
import {blankStyle,decodeTsv,encodeTsv,toA1,type Cell,type Coord,type Workbook} from '../../excel-studio/src/model';
import type {Value} from './api-client';
import {keyFor,type Tab} from './store';

export const text=(v:Value|undefined)=>v===undefined?'':v===null?'NULL':typeof v==='object'?JSON.stringify(v):String(v);
export function convert(raw:string,prior:Value):Value {
  if(raw==='NULL')return null;
  if(typeof prior==='number'){const n=Number(raw);if(raw.trim()===''||!Number.isFinite(n))throw new Error('请输入有效数字，或输入 NULL');return n;}
  if(typeof prior==='boolean'){if(!/^(true|false)$/i.test(raw))throw new Error('请输入 true、false 或 NULL');return raw.toLowerCase()==='true';}
  if(prior && typeof prior==='object'){try{return JSON.parse(raw) as Value;}catch{throw new Error('请输入有效 JSON 或 NULL');}}
  return raw;
}
class Adapter extends WorkbookStore {
  rows=1; cols=1;
  constructor(private readonly source:ResultGrid){super({recalc:async b=>b,operate:async(b,op)=>{
    if(op.op!=='dimensions'||op.change.action!=='set_width')return b;
    const s=b.sheets[0];source.widths[op.change.column]=op.change.width;
    return {...b,sheets:[{...s,widths:{...s.widths,[op.change.column]:op.change.width}}]};
  }});}
  private constrain(p:Coord){return {row:Math.min(Math.max(p.row,1),this.rows),col:Math.min(Math.max(p.col,1),this.cols)};}
  override locate(p:Coord,extend=false){super.locate(this.constrain(p),extend);this.source.notifySelection();}
  override selectArea(a:Coord,b:Coord){super.selectArea(this.constrain(a),this.constrain(b));this.source.notifySelection();}
  override copy(){const {start,end}=this.range;return encodeTsv(Array.from({length:end.row-start.row+1},(_,r)=>Array.from({length:end.col-start.col+1},(_,c)=>this.sheet.cells[toA1({row:start.row+r,col:start.col+c})]?.display??'')));}
  override async paste(raw:string){const start=this.active;const changes=decodeTsv(raw).flatMap((row,r)=>row.map((v,c)=>({p:{row:start.row+r,col:start.col+c},raw:v})));this.source.changes(changes);}
  override async clear(){const {start,end}=this.range;const changes=[];for(let r=start.row;r<=end.row;r++)for(let c=start.col;c<=end.col;c++)changes.push({p:{row:r,col:c},raw:'NULL'});this.source.changes(changes);}
}
export interface CellInspectInfo {
  readonly column: string;
  readonly rowNumber: number;
  readonly value: string;
  readonly isNull: boolean;
  readonly editable: boolean;
  readonly onEdit?: () => void;
}
export class ResultGrid {
  widths:Record<number,number>={};
  private tab?:Tab;
  private visible:number[]=[];
  private filter='';
  private sort?:{i:number;desc:boolean};
  private draft?:{p:Coord;raw:string};
  private clickTimer?:number;
  private readonly model:Adapter;
  readonly engine:Grid;
  readonly root:HTMLElement;
  constructor(private readonly changed:()=>void,private readonly viewer:(info:CellInspectInfo)=>void,private readonly error:(error:unknown)=>void,private readonly onSelect?:()=>void){
    this.model=new Adapter(this);
    this.engine=new Grid(this.model,{start:(p,raw)=>this.start(p,raw),draft:raw=>{if(this.draft)this.draft.raw=raw;},commit:move=>this.commit(move),cancel:()=>this.cancel(),error,clickCell:p=>this.scheduleInspect(p)});
    this.root=this.engine.root;
    this.root.setAttribute('aria-label','查询结果');
    this.root.addEventListener('pointerdown',e=>{
      const el=e.target instanceof Element?e.target.closest<HTMLElement>('.frozen-plane [data-col]'):null;
      if(el&&e.button===0){e.stopImmediatePropagation();e.preventDefault();const i=Number(el.dataset.col)-1;this.sort={i,desc:this.sort?.i===i?!this.sort.desc:false};this.paint();}
    },{capture:true});
    const observer=new MutationObserver(()=>{
      for(const el of this.root.querySelectorAll<HTMLElement>('[data-address]')){
        const row=Number(el.dataset.row),col=Number(el.dataset.col)-1;
        const idx=this.visible[row-2];const data=idx===undefined?undefined:this.value(idx,col);
        el.classList.toggle('db-null',data===null);
        el.classList.toggle('db-dirty',idx!==undefined&&!!this.tab?.patches.has(keyFor(idx,col)));
        if(row===1&&this.tab?.page?.columns[col]){el.title='按 '+this.tab.page.columns[col].name+' 排序';el.classList.toggle('db-sort',this.sort?.i===col);el.dataset.order=this.sort?.desc?'↓':'↑';}
      }
      for(const el of this.root.querySelectorAll<HTMLElement>('[data-header-row]')){
        const row=Number(el.dataset.headerRow); const label=row===1?'#':row-2<this.visible.length?String((this.tab?.page?.offset??0)+this.visible[row-2]+1):'';
        if(el.textContent!==label)el.textContent=label;
      }
    });
    observer.observe(this.root,{childList:true,subtree:true});
  }
  load(t:Tab|undefined){this.cancel();this.tab=t;this.filter='';this.sort=undefined;this.widths={};this.paint();}
  search(text:string){this.filter=text.toLocaleLowerCase();this.paint();}
  get count(){return this.visible.length;}
  refresh(){this.paint();}
  notifySelection(){this.onSelect?.();}
  private resolveColumnIndex(spec:string|number|undefined):number|undefined{
    const cols=this.tab?.page?.columns;
    if(!cols||!cols.length||spec===undefined||spec==='')return undefined;
    if(typeof spec==='number'){return spec>=1&&spec<=cols.length?Math.floor(spec):undefined;}
    const trimmed=spec.trim();
    if(!trimmed)return undefined;
    const exact=cols.findIndex(c=>c.name.toLowerCase()===trimmed.toLowerCase());
    if(exact>=0)return exact+1;
    if(/^\d+$/.test(trimmed)){const n=Number(trimmed);if(n>=1&&n<=cols.length)return n;}
    if(/^[A-Za-z]{1,3}$/.test(trimmed)){
      let n=0;for(const ch of trimmed.toUpperCase())n=n*26+ch.charCodeAt(0)-64;
      if(n>=1&&n<=cols.length)return n;
    }
    const partial=cols.findIndex(c=>c.name.toLowerCase().includes(trimmed.toLowerCase()));
    return partial>=0?partial+1:undefined;
  }
  selectTarget(target:{column?:string|number;row?:number;range?:string;findColumn?:string;findValue?:string}):string|null{
    const page=this.tab?.page;
    if(!page||!page.columns.length)return null;
    const parseA1=(addr:string):Coord|undefined=>{
      const m=/^([A-Za-z]+)(\d+)$/.exec(addr.trim());
      if(!m)return undefined;
      let col=0;for(const ch of m[1].toUpperCase())col=col*26+ch.charCodeAt(0)-64;
      return {row:Number(m[2]),col};
    };
    if(target.range){
      const raw=target.range.trim();
      const colSpan=/^([A-Za-z]+):([A-Za-z]+)$/.exec(raw);
      if(colSpan){
        const c1=this.resolveColumnIndex(colSpan[1]),c2=this.resolveColumnIndex(colSpan[2]);
        if(c1&&c2){this.model.selectArea({row:1,col:c1},{row:this.model.rows,col:c2});this.engine.reveal({row:Math.min(2,this.model.rows),col:c1},true);return `${page.columns[c1-1]?.name??raw}`;}
      }
      const rowSpan=/^(\d+):(\d+)$/.exec(raw);
      if(rowSpan){
        const r1=Math.min(this.model.rows,Math.max(1,Number(rowSpan[1])+1));
        const r2=Math.min(this.model.rows,Math.max(1,Number(rowSpan[2])+1));
        this.model.selectArea({row:r1,col:1},{row:r2,col:this.model.cols});this.engine.reveal({row:r1,col:1},true);return `第 ${rowSpan[1]}-${rowSpan[2]} 行`;
      }
      const [a,b]=raw.split(':');
      const p1=parseA1(a),p2=b?parseA1(b):p1;
      if(p1&&p2){this.model.selectArea(p1,p2);this.engine.reveal(p1,true);return raw;}
    }
    const col=this.resolveColumnIndex(target.column);
    let dataRowIndex:number|undefined;
    if(typeof target.row==='number'&&Number.isFinite(target.row)&&target.row>=1){
      dataRowIndex=Math.min(this.visible.length,Math.floor(target.row))-1;
    }else if(target.findValue){
      const needle=target.findValue.toLowerCase();
      const searchCol=this.resolveColumnIndex(target.findColumn)??col;
      const foundIdx=this.visible.findIndex(rIdx=>{
        if(searchCol!==undefined)return text(this.value(rIdx,searchCol-1)).toLowerCase().includes(needle);
        return page.columns.some((_,ci)=>text(this.value(rIdx,ci)).toLowerCase().includes(needle));
      });
      if(foundIdx>=0)dataRowIndex=foundIdx;
    }
    if(col!==undefined&&dataRowIndex!==undefined&&dataRowIndex>=0){
      const gridRow=dataRowIndex+2;
      this.model.locate({row:gridRow,col});
      this.engine.reveal({row:gridRow,col},true);
      return `${page.columns[col-1]?.name??''} 第 ${dataRowIndex+1} 行`;
    }
    if(col!==undefined){
      this.model.selectArea({row:1,col},{row:this.model.rows,col});
      this.engine.reveal({row:Math.min(2,this.model.rows),col},true);
      return `列 ${page.columns[col-1]?.name??col}`;
    }
    if(dataRowIndex!==undefined&&dataRowIndex>=0){
      const gridRow=dataRowIndex+2;
      this.model.selectArea({row:gridRow,col:1},{row:gridRow,col:this.model.cols});
      this.engine.reveal({row:gridRow,col:1},true);
      return `第 ${dataRowIndex+1} 行`;
    }
    return null;
  }
  snapshot(maxRows=15,maxCellChars=160){
    const page=this.tab?.page;
    if(!page)return {columns:[] as string[],totalVisible:0,selectedRowCount:0,selectedRows:[] as Record<string,string>[],sampleRows:[] as Record<string,string>[]};
    const columns=page.columns.map(c=>c.name);
    const clip=(raw:string)=>raw.length>maxCellChars?raw.slice(0,maxCellChars)+'…':raw;
    const rowObj=(idx:number)=>Object.fromEntries(columns.map((name,c)=>[name,clip(text(this.value(idx,c)))]));
    const {start,end}=this.model.range;
    const picked:number[]=[];
    for(let r=Math.max(2,start.row);r<=end.row;r++){
      const idx=this.visible[r-2];
      if(idx!==undefined&&!picked.includes(idx))picked.push(idx);
    }
    const selectedRows=picked.slice(0,maxRows).map(rowObj);
    const sampleRows=picked.length>=2?[]:this.visible.slice(0,Math.min(10,maxRows)).map(rowObj);
    return {columns,totalVisible:this.visible.length,selectedRowCount:picked.length,selectedRows,sampleRows};
  }
  private value(r:number,c:number){const p=this.tab?.patches.get(keyFor(r,c));return p?p.value:this.tab?.page?.rows[r]?.[c];}
  private paint(){
    const page=this.tab?.page;
    this.visible=(page?.rows??[]).map((_,i)=>i).filter(i=>!this.filter||page!.rows[i].some((_,c)=>text(this.value(i,c)).toLocaleLowerCase().includes(this.filter)));
    const s=this.sort;
    if(s&&page)this.visible.sort((a,b)=>{const x=this.value(a,s.i),y=this.value(b,s.i);const cmp=x===null?y===null?0:1:y===null?-1:typeof x==='number'&&typeof y==='number'?x-y:text(x).localeCompare(text(y),undefined,{numeric:true});return s.desc?-cmp:cmp;});
    const cells:Record<string,Cell>={};const defaults:Record<number,number>={};
    (page?.columns??[]).forEach((c,i)=>{const col=i+1;defaults[col]=Math.min(36,Math.max(14,c.name.length+6));cells[toA1({row:1,col})]={raw:c.name,value:c.name,display:c.name,formula:null,style:{...blankStyle(),bold:true}};});
    this.visible.forEach((r,i)=>page!.columns.forEach((_,c)=>{
      const v=this.value(r,c);const display=text(v);
      cells[toA1({row:i+2,col:c+1})]={raw:display,value:v===undefined||typeof v==='object'?(v===null?null:display):v,display,formula:null,style:{...blankStyle(),italic:v===null}};
    }));
    const old=this.model.active;
    const wb:Workbook={sheets:[{name:'Result',cells,widths:{...defaults,...this.widths},frozen:true,merges:[]}]};
    this.model.rows=this.visible.length+1;this.model.cols=page?.columns.length||1;
    const grid=this.engine.viewport;const top=grid.scrollTop,left=grid.scrollLeft;
    this.model.load(wb,'结果');
    this.engine.setBounds(this.model.rows,this.model.cols);
    this.model.locate(old);grid.scrollTo(left,top);
    this.root.setAttribute('aria-rowcount',String(this.model.rows));this.root.setAttribute('aria-colcount',String(this.model.cols));
  }
  private clearPendingInspect(){if(this.clickTimer!==undefined){window.clearTimeout(this.clickTimer);this.clickTimer=undefined;}}
  private buildInspectInfo(p:Coord):CellInspectInfo|undefined{
    if(p.row<2)return undefined;
    const r=this.visible[p.row-2];const column=this.tab?.page?.columns[p.col-1];
    if(r===undefined||!column)return undefined;
    const v=this.value(r,p.col-1);
    const editable=!!this.tab?.page?.editable_table?.columns.includes(column.name);
    const raw=text(v);
    return {
      column:column.name,
      rowNumber:(this.tab?.page?.offset??0)+r+1,
      value:raw,
      isNull:v===null,
      editable,
      onEdit:editable?()=>{this.draft={p,raw};this.engine.openEditor(p,raw,true);}:undefined,
    };
  }
  private scheduleInspect(p:Coord){
    this.clearPendingInspect();
    const info=this.buildInspectInfo(p);
    if(!info)return;
    // Read-only cells open immediately on click; editable cells wait briefly so
    // double-click inline editing stays seamless.
    if(!info.editable){this.viewer(info);return;}
    this.clickTimer=window.setTimeout(()=>{
      this.clickTimer=undefined;
      if(!this.draft)this.viewer(info);
    },230);
  }
  private start(p:Coord,raw?:string){
    this.clearPendingInspect();
    const info=this.buildInspectInfo(p);
    if(!info)return;
    const r=this.visible[p.row-2];
    const v=this.value(r,p.col-1);
    if(raw===undefined&&(typeof v==='object'&&v!==null||info.value.length>140)){this.viewer(info);return;}
    if(!info.editable)return;
    this.draft={p,raw:raw??info.value};this.engine.openEditor(p,this.draft.raw,raw===undefined);
  }
  private cancel(){this.clearPendingInspect();this.draft=undefined;this.engine.closeEditor();}
  private commit(move?:Coord){const d=this.draft;if(!d)return;this.cancel();try{this.changes([d]);if(move)this.model.locate({row:d.p.row+move.row,col:d.p.col+move.col});}catch(e){this.error(e);}}
  changes(changes:{p:Coord;raw:string}[]){
    const t=this.tab,p=t?.page,e=p?.editable_table;
    if(!t||!p||!e)throw new Error('当前结果不可编辑');
    if(changes.length>1000)throw new Error('一次最多编辑 1000 个单元格');
    const actions=changes.map(({p:pos,raw})=>{
      const row=this.visible[pos.row-2], col=pos.col-1, column=p.columns[col]?.name;
      if(row===undefined||!column||!e.columns.includes(column))throw new Error('选区含不可编辑的单元格');
      const values=p.rows[row],previous=values[col];const value=convert(raw,previous);
      const keys=Object.fromEntries(e.primary_key.map(k=>[k,values[p.columns.findIndex(c=>c.name===k)]]));
      if(Object.values(keys).some(v=>v===null||v===undefined))throw new Error('当前行的主键为空');
      return {key:keyFor(row,col),patch:{column,value,previous,keys}};
    });
    for(const {key,patch} of actions){if(JSON.stringify(patch.value)===JSON.stringify(patch.previous))t.patches.delete(key);else t.patches.set(key,patch);}
    this.paint();this.changed();
  }
}
