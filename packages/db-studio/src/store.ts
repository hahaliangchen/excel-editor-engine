import type {Connection,Detail,Patch,ResultPage,Table,Target} from './api-client';
export interface Source {key:string;name:string;target:Target;connection?:Connection;readOnly:boolean;schema?:string;schemas?:string[];tables:Map<string,Table[]>}
export interface Tab {id:number;label:string;sql:string;executed:string;source?:Source;detail?:Detail;plan?:ResultPage;page?:ResultPage;patches:Map<string,Patch>;view:'data'|'structure'|'plan';limit:number;schema?:string;table?:string}
export class Store {
  sources:Source[]=[];
  tabs:Tab[]=[];
  active=0;
  selected?:Source;
  private serial=0;
  get tab(){return this.tabs.find(t=>t.id===this.active)!;}
  open(options:Partial<Tab>={}) {
    const id=++this.serial;
    const t:Tab={id,label:'查询 '+id,sql:'',executed:'',source:this.selected,patches:new Map(),view:'data',limit:200,...options};
    this.tabs.push(t);this.active=id;return t;
  }
  close(t:Tab){const index=this.tabs.indexOf(t);this.tabs.splice(index,1);if(!this.tabs.length)this.open();else if(this.active===t.id)this.active=this.tabs[Math.min(index,this.tabs.length-1)].id;}
}
export const quote=(kind:string,s:string)=>{const mark=kind==='mysql'?String.fromCharCode(96):'"';return mark+s.replaceAll(mark,mark+mark)+mark;};
export const keyFor=(r:number,c:number)=>r+':'+c;
