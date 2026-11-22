import {MAX_SELECTION, blankWorkbook, bounds, clamp, contains, decodeTsv, encodeTsv, fromA1, fromRaw, home, overlaps, parseRange, rangeA1, rangeSize, sameCoord, sheetMerges, toA1, validCoord, type CellStyle, type Coord, type DimensionChange, type Operation, type Range, type Selection, type Sheet, type SheetChange, type Workbook} from './model';

export interface Engine {
  recalc(workbook: Workbook): Promise<Workbook>;
  operate(workbook: Workbook, operation: Operation): Promise<Workbook>;
}
interface State { workbook: Workbook; index: number; selection: Selection; title: string }
export interface Stats {count: number; numericCount: number; sum: number; average: number | null; min: number | null; max: number | null}
export type Change = 'data' | 'selection' | 'sheet' | 'busy' | 'load';
const MAX_HISTORY = 80;
const resetSelection = (): Selection => ({anchor: home(), focus: home()});

export function aggregate(sheet: Sheet, range: Range): Stats {
  let count=0, numericCount=0, sum=0, min=Infinity, max=-Infinity;
  const add = (a: string) => {
    const c = sheet.cells[a];
    if (!c || (c.raw === '' && c.value === null)) return;
    count++;
    if (typeof c.value !== 'number' || !Number.isFinite(c.value)) return;
    numericCount++; sum += c.value; min = Math.min(min,c.value); max = Math.max(max,c.value);
  };
  if (rangeSize(range) <= 2048) {
    for (let row=range.start.row; row<=range.end.row; row++) for (let col=range.start.col; col<=range.end.col; col++) add(toA1({row,col}));
  } else for (const a of Object.keys(sheet.cells)) { const p = fromA1(a); if (p && contains(range,p)) add(a); }
  return {count,numericCount,sum,average:numericCount ? sum/numericCount : null,min:numericCount ? min : null,max:numericCount ? max : null};
}

export class WorkbookStore {
  private state: State = {workbook:blankWorkbook(),index:0,selection:resetSelection(),title:'未命名工作簿'};
  private previous: State[] = [];
  private future: State[] = [];
  private listeners = new Set<(change: Change) => void>();
  private queue: Promise<unknown> = Promise.resolve();
  private pending = 0;
  private epoch = 0;
  constructor(private readonly engine: Engine) {}
  get workbook() { return this.state.workbook; }
  get title() { return this.state.title; }
  get index() { return this.state.index; }
  get sheet() { return this.workbook.sheets[this.index]; }
  get selection() { return this.state.selection; }
  get range(): Range {
    let r = bounds(this.selection.anchor,this.selection.focus);
    const merges = sheetMerges(this.sheet);
    // Expand through any intersecting merge, including chains at a selection's edges.
    let changed = true;
    while (changed) {
      changed = false;
      for (const m of merges) if (overlaps(r,m) && (!contains(r,m.start) || !contains(r,m.end))) {
        r = {start: {row:Math.min(r.start.row,m.start.row),col:Math.min(r.start.col,m.start.col)},end:{row:Math.max(r.end.row,m.end.row),col:Math.max(r.end.col,m.end.col)}}; changed = true;
      }
    }
    return r;
  }
  get active() { return this.selection.anchor; }
  get cell() { return this.sheet.cells[toA1(this.active)]; }
  get busy() { return this.pending > 0; }
  get canUndo() { return this.previous.length > 0 && !this.busy; }
  get canRedo() { return this.future.length > 0 && !this.busy; }
  subscribe(listener: (change: Change) => void) { this.listeners.add(listener); return () => this.listeners.delete(listener); }
  private notify(change: Change) { for (const listener of this.listeners) listener(change); }
  idle() { return this.queue; }
  load(workbook: Workbook, title: string) {
    this.epoch++; this.previous = []; this.future = [];
    this.state = {workbook,index:0,selection:resetSelection(),title}; this.notify('load');
  }
  switchSheet(index: number) {
    if (!this.workbook.sheets[index] || index === this.index) return;
    this.state = {...this.state,index,selection:resetSelection()}; this.notify('sheet');
  }
  locate(coord: Coord, extend = false) {
    const p = clamp(coord);
    const merge = sheetMerges(this.sheet).find(r => contains(r,p));
    const focus = merge?.start ?? p;
    const anchor = extend ? this.active : focus;
    if (sameCoord(this.selection.anchor,anchor) && sameCoord(this.selection.focus,focus)) return;
    this.state = {...this.state,selection:{anchor,focus}}; this.notify('selection');
  }
  selectArea(start: Coord, end: Coord) {
    const anchor = clamp(start), focus = clamp(end);
    const merge = sheetMerges(this.sheet).find(r => contains(r,anchor));
    this.state = {...this.state,selection:{anchor:merge?.start ?? anchor,focus}}; this.notify('selection');
  }
  private transact(transform: (state: State) => Promise<Workbook> | Workbook, focusSheet?: string) {
    const epoch = this.epoch;
    const origin = this.state;
    this.pending++; this.notify('busy');
    const result = this.queue.then(async () => {
      if (epoch !== this.epoch) return;
      const before = this.state;
      const result = await transform(before);
      if (epoch !== this.epoch || result === before.workbook) return;
      const current = this.state;
      this.previous.push({...before,index:before.workbook.sheets.some(s => s.name === origin.workbook.sheets[origin.index]?.name) ? before.workbook.sheets.findIndex(s => s.name === origin.workbook.sheets[origin.index]?.name) : before.index,selection:origin.selection});
      if (this.previous.length > MAX_HISTORY) this.previous.shift();
      this.future = [];
      const oldName = current.workbook.sheets[current.index].name;
      const newIndex = result.sheets.findIndex(s => s.name === (focusSheet ?? oldName));
      const index = newIndex === -1 ? Math.min(current.index,result.sheets.length - 1) : newIndex;
      const switched = oldName !== result.sheets[index].name;
      this.state = {...current,workbook:result,index,selection:switched ? resetSelection() : current.selection};
      this.notify(switched ? 'sheet' : 'data');
    }).finally(() => { this.pending--; this.notify('busy'); });
    this.queue = result.catch(() => {});
    return result;
  }
  undo() {
    if (!this.canUndo) return;
    this.future.push(this.state); this.state = this.previous.pop()!; this.notify('data');
  }
  redo() {
    if (!this.canRedo) return;
    this.previous.push(this.state); this.state = this.future.pop()!; this.notify('data');
  }
  write(cells: Record<string,string>, sheetName = this.sheet.name) {
    const writes = Object.entries(cells);
    if (writes.length > MAX_SELECTION || writes.some(([a,v]) => !fromA1(a) || v.length > 32767 || /[\x00-\x08\x0b\x0c\x0e-\x1f]/.test(v))) return Promise.reject(new Error('单元格内容或选区超出限制'));
    return this.transact(({workbook}) => {
      const index = workbook.sheets.findIndex(s => s.name === sheetName);
      if (index < 0) throw new Error('工作表已不存在');
      const sheet = workbook.sheets[index];
      const next = {...sheet.cells};
      let changed = false;
      const merges = sheetMerges(sheet);
      for (const [input,raw] of writes) {
        const coord = fromA1(input)!;
        const merge = merges.find(r => contains(r,coord));
        if (merge && !sameCoord(merge.start,coord)) continue;
        const a = toA1(coord);
        if ((next[a]?.raw ?? '') === raw) continue;
        next[a] = fromRaw(raw,next[a]); changed = true;
      }
      if (!changed) return workbook;
      const sheets = workbook.sheets.slice(); sheets[index] = {...sheet,cells:next};
      return this.engine.recalc({...workbook,sheets});
    });
  }
  clear() {
    const cells: Record<string,string> = {};
    const range = this.range;
    for (const a of Object.keys(this.sheet.cells)) { const p=fromA1(a); if (p && contains(range,p)) cells[a]=''; }
    return this.write(cells);
  }
  paste(text: string) {
    const start = this.active;
    const rows = decodeTsv(text);
    const cells: Record<string,string> = {};
    let end = start;
    for (let r=0;r<rows.length;r++) for (let c=0;c<rows[r].length;c++) {
      const p={row:start.row+r,col:start.col+c};
      if (!validCoord(p)) continue;
      cells[toA1(p)] = rows[r][c]; end = {row:Math.max(end.row,p.row),col:Math.max(end.col,p.col)};
    }
    if (!sameCoord(start,end)) this.selectArea(start,end);
    return this.write(cells);
  }
  copy(): string {
    const range = this.range;
    if (rangeSize(range)>MAX_SELECTION) throw new Error('选区太大，无法复制');
    const rows = [];
    for(let row=range.start.row;row<=range.end.row;row++) {
      const line=[];
      for(let col=range.start.col;col<=range.end.col;col++) line.push(this.sheet.cells[toA1({row,col})]?.raw ?? '');
      rows.push(line);
    }
    return encodeTsv(rows);
  }
  format(style?: Partial<CellStyle>, merge?: boolean) {
    const sheet = this.sheet.name, range = rangeA1(this.range);
    return this.transact(({workbook}) => this.engine.operate(workbook,{op:'format',sheet,range,style,merge}));
  }
  dimensions(change: DimensionChange) {
    const sheet = this.sheet.name;
    return this.transact(({workbook}) => this.engine.operate(workbook,{op:'dimensions',sheet,change}));
  }
  manage(change: SheetChange) {
    return this.transact(({workbook}) => this.engine.operate(workbook,{op:'sheets',change}),change.action==='delete' ? undefined : change.name);
  }
  jump(input: string) {
    const r = parseRange(input);
    if (!r) return false;
    this.selectArea(r.start,r.end); return true;
  }
}
