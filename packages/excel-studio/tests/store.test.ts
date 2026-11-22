import {describe,expect,it,vi} from 'vitest';
import {apiBase} from '../src/api-client';
import {COLS,ROWS,blankSheet,blankWorkbook,bounds,decodeTsv,encodeTsv,fromA1,fromRaw,parseRange,toA1,type Workbook} from '../src/model';
import {WorkbookStore,aggregate,type Engine} from '../src/store';

const fixture=():Workbook=>({sheets:[{...blankSheet('Sales'),cells:{A1:fromRaw('12'),A2:fromRaw('20'),A3:fromRaw('North'),C4:fromRaw('100')}},blankSheet('Total')]});
const engine:Engine={recalc:async w=>w,operate:async(w)=>w};
const setup=()=>{const s=new WorkbookStore(engine);s.load(fixture(),'example');return s;};

describe('Excel coordinates and clipboard',()=>{
  it('round-trips absolute, lowercase, and edge coordinates',()=>{
    expect(fromA1('$aa$301')).toEqual({col:27,row:301});expect(toA1({col:COLS,row:ROWS})).toBe('XFD1048576');
    expect(fromA1('XFE1')).toBeNull();expect(fromA1('A0')).toBeNull();expect(fromA1('A1048577')).toBeNull();
    expect(parseRange('C4:A1')).toEqual({start:{col:1,row:1},end:{col:3,row:4}});expect(parseRange('A1:C3:H3')).toBeNull();
  });
  it('keeps formulas, empty columns, and quoted newlines in TSV',()=>{
    const rows=[['=SUM(A1:A2)','a"b',''],['one\ntwo','a\tb','0042']];
    expect(decodeTsv(encodeTsv(rows)+'\r\n')).toEqual(rows);
    expect(decodeTsv('1\t2\r\n3\t4')).toEqual([['1','2'],['3','4']]);
    expect(fromRaw("'0042").value).toBe('0042');expect(fromRaw('0x11').value).toBe('0x11');
  });
  it('locates the local-service API regardless of bundled resource depth',()=>{
    expect(apiBase(new URL('http://127.0.0.1:4173/')).href).toBe('http://127.0.0.1:8083/api/');
    for(const suffix of ['index.html','page/index.html','assets/nested/index.html']){
      expect(apiBase(new URL('http://localhost:8090/agent/skill-packages/excel/pages/studio/'+suffix)).href).toBe('http://localhost:8090/agent/skill-packages/excel/local-api/api/');
    }
  });
});

describe('workbook transactions',()=>{
  it('writes a range without mutating snapshots, then restores it in one undo',async()=>{
    const s=setup();const original=s.workbook;
    await s.paste('51\t=SUM(A1:A2)\r\n82\t99');
    expect(s.sheet.cells.A1.value).toBe(51);expect(s.sheet.cells.B1.formula).toBe('SUM(A1:A2)');
    expect(original.sheets[0].cells.A1.value).toBe(12);expect(s.copy()).toBe('51\t=SUM(A1:A2)\r\n82\t99');
    s.undo();expect(s.workbook).toBe(original);s.redo();expect(s.sheet.cells.A2.value).toBe(82);
    s.undo();await s.write({A2:'10'});expect(s.canRedo).toBe(false);
  });
  it('serializes asynchronous changes without losing intervening edits',async()=>{
    const recalc=vi.fn(async(w:Workbook)=>w);
    const s=new WorkbookStore({...engine,recalc});s.load(fixture(),'test');
    await Promise.all([s.write({A1:'41'}),s.write({A2:'42'})]);
    expect(recalc.mock.calls[1][0].sheets[0].cells.A1.value).toBe(41);
    expect(s.sheet.cells.A2.value).toBe(42);s.undo();expect(s.sheet.cells.A2.value).toBe(20);
  });
  it('rejects a failed operation without adding undo history',async()=>{
    const s=new WorkbookStore({...engine,recalc:async()=>{throw new Error('failed');}});s.load(fixture(),'test');
    await expect(s.write({A1:'40'})).rejects.toThrow('failed');expect(s.canUndo).toBe(false);expect(s.busy).toBe(false);expect(s.sheet.cells.A1.value).toBe(12);
  });
  it('does not let a late response overwrite another workbook',async()=>{
    let unblock!:()=>void;
    const blocker=new Promise<void>(resolve=>unblock=resolve);
    const s=new WorkbookStore({...engine,recalc:async w=>{await blocker;return w;}});s.load(fixture(),'test');
    const change=s.write({A1:'40'});await Promise.resolve();const replacement=blankWorkbook();s.load(replacement,'fresh');unblock();await change;
    expect(s.workbook).toBe(replacement);expect(s.canUndo).toBe(false);
  });
  it('tracks numeric and populated selections independently, including sparse large ranges',()=>{
    const s=setup();s.selectArea({row:1,col:1},{row:3,col:1});
    expect(aggregate(s.sheet,s.range)).toEqual({count:3,numericCount:2,sum:32,average:16,min:12,max:20});
    expect(aggregate(s.sheet,bounds({row:1,col:1},{row:ROWS,col:COLS}))).toEqual({count:4,numericCount:3,sum:132,average:44,min:12,max:100});
    s.switchSheet(1);expect(aggregate(s.sheet,s.range)).toEqual({count:0,numericCount:0,sum:0,average:null,min:null,max:null});
  });
  it('selects the entire merge and writes only its anchor',async()=>{
    const w=fixture();w.sheets[0].merges=['A1:B1'];const s=new WorkbookStore(engine);s.load(w,'test');
    s.locate({row:1,col:2});expect(s.range).toEqual(parseRange('A1:B1'));
    await s.write({A1:'new',B1:'hidden'});expect(s.sheet.cells.A1.value).toBe('new');expect(s.sheet.cells.B1).toBeUndefined();
  });
  it('sends row/column commands and sheet operations through the same history',async()=>{
    const operate=vi.fn<Engine['operate']>(async(w,op)=>{
      if(op.op==='sheets'&&op.change.action==='new')return {...w,sheets:[...w.sheets,blankSheet(op.change.name)]};
      return {...w,sheets:[{...w.sheets[0],widths:{1:24}},...w.sheets.slice(1)]};
    });
    const s=new WorkbookStore({...engine,operate});s.load(fixture(),'test');
    await s.dimensions({action:'insert_rows',at:2});expect(operate.mock.calls[0][1]).toEqual({op:'dimensions',sheet:'Sales',change:{action:'insert_rows',at:2}});
    await s.dimensions({action:'delete_columns',at:2,count:3});expect(operate.mock.calls[1][1]).toMatchObject({change:{action:'delete_columns',at:2,count:3}});
    await s.manage({action:'new',name:'Forecast'});expect(s.index).toBe(2);s.undo();expect(s.workbook.sheets).toHaveLength(2);s.redo();expect(s.sheet.name).toBe('Forecast');
  });
});
