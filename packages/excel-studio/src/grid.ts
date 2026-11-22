import {COLS, ROWS, bounds, columnLabel, contains, overlaps, sameCoord, sheetMerges, toA1, type Coord, type Range, type Sheet} from './model';
import type {WorkbookStore} from './store';

const ROW_H = 27;
const TOP = 28;
const SIDE = 49;
const DEFAULT_WIDTH = 114;
const widthToPx = (width: number) => Math.max(12,Math.round(width * 7 + 5));
const pxToWidth = (px: number) => (px - 5) / 7;
const el = (tag: string, className: string) => Object.assign(document.createElement(tag),{className});
export interface GridActions {
  start(coord: Coord, text?: string): void;
  draft(text: string): void;
  commit(move?: {row: number; col: number}): void;
  cancel(): void;
  error(error: unknown): void;
}

export class Grid {
  readonly root = el('div','grid');
  readonly viewport = el('div','grid-viewport');
  private readonly spacer = el('div','grid-spacer');
  private readonly plane = el('div','cell-plane');
  private readonly columnHeads = el('div','axis column-heads');
  private readonly rowHeads = el('div','axis row-heads');
  private readonly corner = el('button','grid-corner') as HTMLButtonElement;
  private readonly frozen = el('div','frozen-plane');
  private readonly input = el('input','inline-edit') as HTMLInputElement;
  private offsets = new Float64Array(COLS+1);
  private currentSheet?: Sheet;
  private merges: Range[] = [];
  private frame = 0;
  private dragFrame = 0;
  private drag?: {mode: 'cell'|'row'|'col'; anchor: Coord; id: number; x:number;y:number};
  private resize?: {col:number; x:number; start:number; width:number; id:number};
  private editorAt?: Coord;
  private lastTap?: {coord:Coord;time:number};
  private readonly scrolls = new Map<number,{left:number;top:number}>();
  private lastIndex = 0;

  constructor(private readonly store: WorkbookStore, private readonly actions: GridActions) {
    this.root.setAttribute('role','grid');
    this.root.setAttribute('aria-label','电子表格');
    this.root.setAttribute('aria-rowcount',String(ROWS));
    this.root.setAttribute('aria-colcount',String(COLS));
    this.viewport.tabIndex=0;
    this.viewport.setAttribute('aria-label','单元格');
    this.corner.type='button'; this.corner.title='选择整张工作表'; this.corner.setAttribute('aria-label',this.corner.title);
    this.corner.addEventListener('click',()=>{this.store.selectArea({row:1,col:1},{row:ROWS,col:COLS});this.focus();});
    this.spacer.append(this.plane,this.input);this.viewport.append(this.spacer);
    this.root.append(this.viewport,this.frozen,this.columnHeads,this.rowHeads,this.corner);
    this.input.type='text';this.input.hidden=true;this.input.setAttribute('aria-label','编辑单元格');this.input.spellcheck=false;
    this.input.addEventListener('input',()=>this.actions.draft(this.input.value));
    this.input.addEventListener('keydown',e=>{
      if (e.isComposing) return;
      if (e.key==='Escape') {e.preventDefault();e.stopPropagation();this.actions.cancel();this.focus();}
      if (e.key==='Enter' || e.key==='Tab') {e.preventDefault();e.stopPropagation();this.actions.commit(e.key==='Enter'?{row:e.shiftKey?-1:1,col:0}:{row:0,col:e.shiftKey?-1:1});this.focus();}
    });
    this.input.addEventListener('blur',e=>{if (this.editorAt && !(e.relatedTarget instanceof HTMLElement && (e.relatedTarget.id==='fx-input' || this.input.hidden))) this.actions.commit();});
    this.viewport.addEventListener('scroll',()=>this.schedule(),{passive:true});
    this.root.addEventListener('pointerdown',e=>this.pointerDown(e));
    this.root.addEventListener('keydown',e=>this.key(e));
    this.root.addEventListener('copy',e=>{if(e.target===this.input)return;try{e.clipboardData?.setData('text/plain',this.store.copy());e.preventDefault();}catch(error){this.actions.error(error);}});
    this.root.addEventListener('paste',e=>{if(e.target===this.input || !e.clipboardData?.types.includes('text/plain'))return;e.preventDefault();this.store.paste(e.clipboardData.getData('text/plain')).catch(this.actions.error);});
    window.addEventListener('pointermove',e=>this.pointerMove(e),{passive:false});
    window.addEventListener('pointerup',e=>this.endPointer(e));
    window.addEventListener('pointercancel',e=>this.endPointer(e,true));
    new ResizeObserver(()=>this.schedule()).observe(this.viewport);
    store.subscribe(change=>{
      if (change==='busy') return;
      if (change==='sheet' || change==='load' || this.lastIndex!==store.index) {
        if (change==='load') this.scrolls.clear();
        else this.scrolls.set(this.lastIndex,{left:this.viewport.scrollLeft,top:this.viewport.scrollTop});
        this.lastIndex=store.index;
        this.rebuild();
        this.viewport.scrollTo(this.scrolls.get(store.index) ?? {left:0,top:0});
      } else if (this.currentSheet!==store.sheet) this.rebuild();
      this.schedule();
    });
    this.rebuild();this.schedule();
  }
  focus() {this.viewport.focus({preventScroll:true});}
  private get baseRow() {return this.store.sheet.frozen ? 2 : 1;}
  private get frozenHeight() {return this.store.sheet.frozen ? ROW_H : 0;}
  private rebuild() {
    const sheet = this.store.sheet;
    this.currentSheet=sheet;this.merges=sheetMerges(sheet);
    this.root.style.setProperty('--frozen',this.frozenHeight+'px');
    this.frozen.hidden=!sheet.frozen;
    this.rebuildWidths();
    this.spacer.style.height=((ROWS-this.baseRow+1)*ROW_H)+'px';
  }
  private rebuildWidths() {
    const widths = this.store.sheet.widths;
    for(let c=1;c<=COLS;c++) this.offsets[c]=this.offsets[c-1]+(this.resize?.col===c ? this.resize.width : widths[c]!==undefined ? widthToPx(widths[c]) : DEFAULT_WIDTH);
    this.spacer.style.width=this.offsets[COLS]+'px';
  }
  private schedule() {if (!this.frame) this.frame=requestAnimationFrame(()=>{this.frame=0;this.render();});}
  private x(col: number) {return this.offsets[col-1];}
  private colAt(px: number) {
    let low=1,high=COLS;
    while (low<high) {const mid=(low+high)>>1;if (this.offsets[mid]>px) high=mid;else low=mid+1;}
    return low;
  }
  private y(row: number){return (row-this.baseRow)*ROW_H;}
  private mergeAt(p: Coord) {return this.merges.find(m=>contains(m,p));}
  private rangeFor(p: Coord) {return this.mergeAt(p) ?? {start:p,end:p};}
  private makeCell(p: Coord, range: Range, selected: Range, frozen: boolean, clipEnd = range.end.row) {
    const a=toA1(p), cell=this.store.sheet.cells[a];
    const block=el('div','grid-cell');
    const x=this.x(p.col)-(frozen ? this.viewport.scrollLeft : 0);
    const y=frozen ? 0 : this.y(Math.max(this.baseRow,p.row));
    const high=(clipEnd-Math.max(frozen ? 1 : this.baseRow,p.row)+1)*ROW_H;
    Object.assign(block.style,{left:x+'px',top:y+'px',width:(this.offsets[range.end.col]-this.x(p.col))+'px',height:high+'px'});
    block.dataset.row=String(p.row);block.dataset.col=String(p.col);block.dataset.address=a;
    block.setAttribute('role','gridcell');block.setAttribute('aria-rowindex',String(p.row));block.setAttribute('aria-colindex',String(p.col));
    if (!sameCoord(range.start,range.end)){block.classList.add('merged');block.setAttribute('aria-colspan',String(range.end.col-range.start.col+1));block.setAttribute('aria-rowspan',String(range.end.row-range.start.row+1));}
    if (overlaps(range, selected)) {block.classList.add('selected');block.setAttribute('aria-selected','true');}
    if (sameCoord(this.store.active,p)) block.classList.add('active');
    if (cell) {
      if (cell.style.bold) block.classList.add('bold');if(cell.style.italic)block.classList.add('italic');
      if (cell.error) block.classList.add('cell-error');
      if (cell.style.fill && /^#[\da-f]{6}$/i.test(cell.style.fill)) {block.classList.add('filled');block.style.setProperty('--cell-fill',cell.style.fill);}
      if (cell.style.color && /^#[\da-f]{6}$/i.test(cell.style.color)) {
        // Imported theme-black uses the workbench foreground on an unfilled cell.
        if (cell.style.fill || !/^#(?:000000|FFFFFF)$/i.test(cell.style.color)) block.style.setProperty('--cell-ink',cell.style.color);
      }
      block.style.textAlign=cell.style.align ?? (typeof cell.value==='number'?'right':typeof cell.value==='boolean'?'center':'left');
      const span=el('span','cell-text');span.textContent=cell.display;block.append(span);
      if (cell.formula) block.dataset.formula=cell.formula;
    }
    return block;
  }
  private render() {
    if (!this.viewport.clientWidth || !this.viewport.clientHeight) return;
    const {scrollLeft:left,scrollTop:top,clientWidth:w,clientHeight:h}=this.viewport;
    const c0=Math.max(1,this.colAt(left)-2),c1=Math.min(COLS,this.colAt(left+w)+2);
    const r0=Math.max(this.baseRow,this.baseRow+Math.floor(top/ROW_H)-3);
    const r1=Math.min(ROWS,this.baseRow+Math.floor((top+h)/ROW_H)+3);
    const selected=this.store.range;
    const windowRange=bounds({row:r0,col:c0},{row:r1,col:c1});
    const matching=this.merges.filter(m=>overlaps(m,windowRange));
    const frag=document.createDocumentFragment();
    for(let r=r0;r<=r1;r++) {
      const rowMerges=matching.filter(m=>m.start.row<=r && r<=m.end.row);
      for(let c=c0;c<=c1;c++) {
        const p={row:r,col:c};if(rowMerges.some(m=>m.start.col<=c && c<=m.end.col))continue;
        frag.append(this.makeCell(p,{start:p,end:p},selected,false));
      }
    }
    for(const m of matching) frag.append(this.makeCell(m.start,m,selected,false));
    this.plane.replaceChildren(frag);
    const cols=document.createDocumentFragment();
    for(let c=c0;c<=c1;c++) {
      const head=el('div','column-head');head.setAttribute('role','columnheader');head.textContent=columnLabel(c);head.dataset.headerCol=String(c);
      if (c>=selected.start.col&&c<=selected.end.col) head.classList.add('axis-active');
      Object.assign(head.style,{left:(this.x(c)-left)+'px',width:(this.offsets[c]-this.x(c))+'px'});
      const grab=el('span','resize-handle');grab.dataset.resize=String(c);grab.title='拖动调整列宽';head.append(grab);cols.append(head);
    }
    this.columnHeads.replaceChildren(cols);
    const rows=document.createDocumentFragment();
    const rowHeader=(r:number,y:number)=>{
      const head=el('div','row-head');head.setAttribute('role','rowheader');head.textContent=String(r);head.dataset.headerRow=String(r);head.style.top=y+'px';
      if(r>=selected.start.row&&r<=selected.end.row)head.classList.add('axis-active');rows.append(head);
    };
    for(let r=r0;r<=r1;r++)rowHeader(r,this.y(r)-top+this.frozenHeight);
    if (this.frozenHeight) {
      rowHeader(1,0);const frozen=document.createDocumentFragment();
      const visible=this.merges.filter(m=>m.start.row===1 && m.start.col<=c1 && m.end.col>=c0);
      for(let c=c0;c<=c1;c++){
        if(visible.some(m=>m.start.col<=c&&c<=m.end.col))continue;
        const p={row:1,col:c};frozen.append(this.makeCell(p,{start:p,end:p},selected,true));
      }
      for(const m of visible)frozen.append(this.makeCell(m.start,m,selected,true,1));
      this.frozen.replaceChildren(frozen);
    }
    this.rowHeads.replaceChildren(rows);
    this.positionEditor();
  }
  private coordinateTarget(target: EventTarget|null): {mode:'cell'|'row'|'col';at:Coord}|null {
    if (!(target instanceof Element))return null;
    const hit=target.closest<HTMLElement>('[data-row],[data-header-row],[data-header-col]');
    if(!hit) return null;
    if(hit.dataset.headerRow) return {mode:'row',at:{row:Number(hit.dataset.headerRow),col:1}};
    if(hit.dataset.headerCol) return {mode:'col',at:{row:1,col:Number(hit.dataset.headerCol)}};
    return {mode:'cell',at:{row:Number(hit.dataset.row),col:Number(hit.dataset.col)}};
  }
  private pointerDown(e:PointerEvent) {
    if (e.button!==0 || e.target===this.input || !(e.target instanceof Element))return;
    const col=Number(e.target.closest<HTMLElement>('[data-resize]')?.dataset.resize);
    if(col){e.preventDefault();this.actions.commit();this.resize={col,x:e.clientX,start:this.offsets[col]-this.x(col),width:this.offsets[col]-this.x(col),id:e.pointerId};this.root.classList.add('resizing');return;}
    const p=this.coordinateTarget(e.target);if(!p)return;
    e.preventDefault();this.actions.commit();this.focus();
    const at=this.rangeFor(p.at).start;
    if(p.mode==='cell' && !e.shiftKey && this.lastTap && sameCoord(this.lastTap.coord,at) && performance.now()-this.lastTap.time<420){
      this.lastTap=undefined;this.store.locate(at);this.actions.start(this.store.active);return;
    }
    this.lastTap=p.mode==='cell' ? {coord:at,time:performance.now()} : undefined;
    const anchor=e.shiftKey?this.store.active:p.at;
    this.drag={mode:p.mode,anchor,id:e.pointerId,x:e.clientX,y:e.clientY};this.selectDrag(p.at);
  }
  private selectDrag(at:Coord) {
    const drag=this.drag;if(!drag)return;
    if(drag.mode==='cell'){
      if (sameCoord(this.store.selection.anchor,drag.anchor) && sameCoord(this.store.selection.focus,at)) return;
      this.store.selectArea(drag.anchor,at);
    } else if(drag.mode==='row') this.store.selectArea({row:drag.anchor.row,col:1},{row:at.row,col:COLS});
    else this.store.selectArea({row:1,col:drag.anchor.col},{row:ROWS,col:at.col});
  }
  private pointerMove(e:PointerEvent) {
    if(this.resize?.id===e.pointerId){e.preventDefault();this.resize.width=Math.min(widthToPx(255),Math.max(35,this.resize.start+e.clientX-this.resize.x));this.rebuildWidths();this.schedule();return;}
    if(this.drag?.id!==e.pointerId)return;
    e.preventDefault();if(Math.hypot(this.drag.x-e.clientX,this.drag.y-e.clientY)>3)this.lastTap=undefined;
    this.drag.x=e.clientX;this.drag.y=e.clientY;this.dragPosition();
  }
  private dragPosition() {
    const drag=this.drag;if(!drag)return;
    const rect=this.viewport.getBoundingClientRect(), right=rect.left+this.viewport.clientWidth,bottom=rect.top+this.viewport.clientHeight;
    const leftOffset=drag.x-rect.left, topOffset=drag.y-rect.top;
    const row=topOffset<0 && this.frozenHeight && drag.y>=rect.top-this.frozenHeight ? 1 : Math.min(ROWS,this.baseRow+Math.floor((this.viewport.scrollTop+Math.max(0,Math.min(topOffset,this.viewport.clientHeight-1)))/ROW_H));
    const col=this.colAt(this.viewport.scrollLeft+Math.max(0,Math.min(leftOffset,this.viewport.clientWidth-1)));
    this.selectDrag({row,col});
    const dx=drag.mode==='row'?0:drag.x>right?18:drag.x<rect.left-SIDE? -18:0;
    const dy=drag.mode==='col'?0:drag.y>bottom?14:drag.y<rect.top-this.frozenHeight-TOP?-14:0;
    if((dx||dy) && !this.dragFrame) this.dragFrame=requestAnimationFrame(()=>{this.dragFrame=0;this.viewport.scrollBy(dx,dy);this.dragPosition();});
  }
  private endPointer(e:PointerEvent,cancelled=false) {
    if(this.drag?.id===e.pointerId){this.drag=undefined;cancelAnimationFrame(this.dragFrame);this.dragFrame=0;}
    if(this.resize?.id===e.pointerId){const {col,start,width}=this.resize;this.resize=undefined;this.root.classList.remove('resizing');this.rebuildWidths();this.schedule();
      if(!cancelled && width!==start)this.store.dimensions({action:'set_width',column:col,width:pxToWidth(width)}).catch(this.actions.error);}
  }
  reveal(p:Coord) {
    const r=this.rangeFor(p), {scrollLeft:left,scrollTop:top,clientWidth:w,clientHeight:h}=this.viewport;
    if(this.x(r.start.col)<left)this.viewport.scrollLeft=this.x(r.start.col);
    else if(this.offsets[r.end.col]>left+w)this.viewport.scrollLeft=Math.max(this.x(r.start.col)-w/2,this.offsets[r.end.col]-w);
    if(p.row<this.baseRow)return;
    if(this.y(p.row)<top)this.viewport.scrollTop=this.y(p.row);
    else if(this.y(p.row)+ROW_H>top+h)this.viewport.scrollTop=this.y(p.row)+ROW_H-h;
  }
  move(delta:{row:number;col:number},extend=false) {
    const p=extend ? this.store.selection.focus : this.store.active;
    const m=this.rangeFor(p);
    const dest={row:(delta.row>0?m.end.row:m.start.row)+delta.row,col:(delta.col>0?m.end.col:m.start.col)+delta.col};
    this.store.locate(dest,extend);this.reveal(this.store.selection.focus);
  }
  private key(e:KeyboardEvent) {
    if(e.target===this.input || e.isComposing || e.target instanceof HTMLButtonElement)return;
    const cmd=e.ctrlKey||e.metaKey;
    const directions:Record<string,{row:number;col:number}>={ArrowUp:{row:-1,col:0},ArrowDown:{row:1,col:0},ArrowLeft:{row:0,col:-1},ArrowRight:{row:0,col:1},PageUp:{row:-Math.floor(this.viewport.clientHeight/ROW_H),col:0},PageDown:{row:Math.floor(this.viewport.clientHeight/ROW_H),col:0},Enter:{row:e.shiftKey?-1:1,col:0},Tab:{row:0,col:e.shiftKey?-1:1}};
    if(Object.hasOwn(directions,e.key)) {e.preventDefault();this.move(directions[e.key],e.shiftKey && !['Enter','Tab'].includes(e.key));}
    else if(e.key==='Home' || e.key==='End') {
      if(!cmd&&e.key==='End')return;
      e.preventDefault();let p={row:cmd?1:this.store.active.row,col:1};
      if(e.key==='End'){
        for(const a of Object.keys(this.store.sheet.cells)) {const cell=this.store.sheet.cells[a];if(!cell.raw) continue;const n=/^(\D+)(\d+)$/.exec(a);if(n)p={row:Math.max(p.row,Number(n[2])),col:Math.max(p.col,[...n[1]].reduce((i,c)=>i*26+c.charCodeAt(0)-64,0))};}
      }
      this.store.locate(p,e.shiftKey);this.reveal(p);
    } else if(e.key==='F2'){e.preventDefault();this.actions.start(this.store.active);}
    else if(e.key==='Delete'||e.key==='Backspace'){e.preventDefault();this.store.clear().catch(this.actions.error);}
    else if(cmd && e.key.toLowerCase()==='a'){e.preventDefault();this.store.selectArea({row:1,col:1},{row:ROWS,col:COLS});}
    else if(!cmd && !e.altKey && e.key.length===1){e.preventDefault();this.actions.start(this.store.active,e.key);}
  }
  openEditor(at:Coord,value:string,select=false) {
    this.reveal(at);this.editorAt=at;this.input.value=value;this.input.hidden=false;this.positionEditor();
    this.input.focus({preventScroll:true});if(select)this.input.select();else this.input.setSelectionRange(value.length,value.length);
  }
  updateEditor(value:string){if(this.editorAt && this.input.value!==value)this.input.value=value;}
  closeEditor(){this.editorAt=undefined;this.input.hidden=true;if(this.input.parentElement!==this.spacer)this.spacer.append(this.input);}
  private positionEditor(){
    const at=this.editorAt;if(!at)return;
    const r=this.rangeFor(at), frozen=at.row<this.baseRow;
    if(frozen && this.input.parentElement!==this.root)this.root.append(this.input);
    else if(!frozen && this.input.parentElement!==this.spacer)this.spacer.append(this.input);
    Object.assign(this.input.style,{left:(this.x(at.col)+(frozen?SIDE-this.viewport.scrollLeft:0))+'px',top:(frozen?TOP:this.y(at.row))+'px',width:Math.max(this.offsets[r.end.col]-this.x(at.col),Math.min(320,this.input.value.length*8+28))+'px',height:ROW_H+'px'});
  }
}
