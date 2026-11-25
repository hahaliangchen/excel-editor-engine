export type Value = null|boolean|number|string|Value[]|{[key:string]:Value};
export type Target = {connection_id:string}|{sqlite_path:string}|{attachment_id:string};
export type Kind = 'sqlite'|'mysql'|'postgres';
export type Env = 'dev'|'staging'|'prod';
export interface Connection {id:string;name:string;driver:Kind;env:Env;read_only:boolean;file_path?:string;host?:string;port?:number;database?:string;username?:string;ssl_mode?:string;has_password?:boolean;notes?:string}
export interface Field {name:string;type:string;nullable:boolean;primary_key?:boolean;default?:string;generated?:boolean}
export interface Detail {schema:string;table:string;kind:string;columns:Field[];primary_key:string[];indexes:{name:string;unique:boolean;columns?:string[];ddl?:string}[];foreign_keys:Value[];ddl?:string}
export interface Table {name:string;kind:string;estimated_rows:number|string|null}
export interface ResultPage {columns:{name:string;type:string}[];rows:Value[][];truncated:boolean;next_offset:number|null;elapsed_ms:number;offset:number;editable_table?:{schema:string;table:string;primary_key:string[];columns:string[]}}
export interface Patch {column:string;keys:Record<string,Value>;previous:Value;value:Value}
export interface ImportInput {xlsx_path?:string;xlsx_base64?:string;sheet?:string;schema?:string;table_name:string;mode:'preview'|'create_and_insert'|'append'}
const page=new URL(location.href);
export const embed=/^(.*\/agent\/skill-packages\/[^/]+)\/pages?\//.exec(page.pathname);
const api=new URL(embed?embed[1]+'/local-api/api/':'/api/',page);
export class Client {
  constructor(public workspace=page.searchParams.get('workspace_root')||''){}
  private async call(path:string,method='GET',data:object={},raw=false):Promise<Response> {
    const url=new URL(path,api);
    const body=this.workspace?{...data,workspace_root:this.workspace}:data;
    if(method==='GET'||method==='DELETE')for(const [k,v] of Object.entries(body))if(v!==undefined)url.searchParams.set(k,String(v));
    const res=await fetch(url,{method,signal:AbortSignal.timeout(65_000),...(method==='POST'?{headers:{'Content-Type':'application/json'},body:JSON.stringify(body)}:{})});
    if(!res.ok) {const error=await res.json().catch(()=>({error:'本地服务请求失败：HTTP '+res.status}));throw new Error(error.error||'服务请求失败');}
    if(!raw && !res.headers.get('content-type')?.includes('application/json'))throw new Error('无法连接数据库服务');
    return res;
  }
  private async json<T>(path:string,method?:string,data?:object):Promise<T> {return (await this.call(path,method,data)).json();}
  list(){return this.json<{connections:Connection[];workspace_sqlite:{path:string}[]}>('connections');}
  save(data:object){return this.json<{connection:Connection}>('connections','POST',data);}
  test(data:object){return this.json<{ok:boolean;version?:string;error?:string}>('connections/test','POST',data);}
  delete(id:string){return this.json('connections/'+encodeURIComponent(id),'DELETE');}
  async schemas(target:Target){return this.json<{schemas:string[];default_schema:string}>('schema','GET',target);}
  async tables(target:Target,schema:string){
    const all:Table[]=[];let offset=0;
    do {const r=await this.json<{tables:Table[];next_offset:number|null}>('schema','GET',{...target,schema,offset,limit:500});all.push(...r.tables);if(r.next_offset===null)break;offset=r.next_offset;}while(all.length<10000);
    return all;
  }
  detail(target:Target,schema:string,table:string){return this.json<Detail>('schema','GET',{...target,schema,table,include_ddl:true});}
  query(target:Target,sql:string,limit:number,offset:number){return this.json<ResultPage>('query','POST',{...target,sql,limit,offset});}
  explain(target:Target,sql:string){return this.json<ResultPage>('explain','POST',{...target,sql});}
  execute(target:Target,args:object){return this.json<{statements?:{sql:string;params:Value[]}[];rows_affected?:number}>('execute','POST',{...target,...args});}
  async download(target:Target,sql:string){return (await this.call('xlsx/export','POST',{...target,sql},true)).blob();}
  exportPath(target:Target,sql:string,output_path:string){return this.json<{path:string;rows:number;truncated:boolean}>('xlsx/export','POST',{...target,sql,output_path});}
  import(target:Target,args:ImportInput,confirm_prod=false){return this.json<{columns?:Field[];rows?:Value[][];row_count?:number;rows_inserted?:number}>('xlsx/import','POST',{...target,...args,confirm_prod});}
  async attachment(blob:Blob){const res=await fetch(new URL('sqlite/attachment',api),{method:'POST',headers:{'Content-Type':'application/octet-stream'},body:blob,signal:AbortSignal.timeout(30_000)});if(!res.ok) throw new Error('无法打开 SQLite 附件');return (await res.json() as {attachment_id:string}).attachment_id;}
}
