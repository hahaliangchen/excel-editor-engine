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
export class ResultGrid {
  widths:Record<number,number>={};
  private tab?:Tab;
  private visible:number[]=[];
  private filter='';
  private sort?:{i:number;desc:boolean};
  private draft?:{p:Coord;raw:string};
  private readonly model:Adapter;
  readonly engine:Grid;
  readonly root:HTMLElement;
  constructor(private readonly changed:()=>void,private readonly viewer:(title:string,value:string)=>void,private readonly error:(error:unknown)=>void,private readonly onSelect?:()=>void){
    this.model=new Adapter(this);
    this.engine=new Grid(this.model,{start:(p,raw)=>this.start(p,raw),draft:raw=>{if(this.draft)this.draft.raw=raw;},commit:move=>this.commit(move),cancel:()=>this.cancel(),error});
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
    this.model.load(wb,'结果');this.model.locate(old);grid.scrollTo(left,top);
    this.root.setAttribute('aria-rowcount',String(this.model.rows));this.root.setAttribute('aria-colcount',String(this.model.cols));
  }
  private start(p:Coord,raw?:string){
    const r=this.visible[p.row-2];const column=this.tab?.page?.columns[p.col-1];
    if(r===undefined||!column)return;
    const v=this.value(r,p.col-1);
    if(raw===undefined&&(typeof v==='object'&&v!==null||text(v).length>140)){this.viewer(column.name,text(v));return;}
    if(!this.tab?.page?.editable_table?.columns.includes(column.name))return;
    this.draft={p,raw:raw??text(v)};this.engine.openEditor(p,this.draft.raw,raw===undefined);
  }
  private cancel(){this.draft=undefined;this.engine.closeEditor();}
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
