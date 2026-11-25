import assert from 'node:assert/strict';
import {access,readFile,realpath,stat} from 'node:fs/promises';
import {constants} from 'node:fs';
import {join,resolve,sep} from 'node:path';
import {manifest} from './manifest.mjs';
import {fixture,rpc,start} from './runtime.mjs';

assert.ok(process.argv[2],'usage: node verify.mjs <bundle directory>');
const dest=await realpath(resolve(process.argv[2]));
const bundle=JSON.parse(await readFile(join(dest,'plugin.json'),'utf8'));
assert.deepEqual(bundle,manifest);
assert.equal(bundle.command_tools.length,9);
const keys=new Set(['name','description','input_schema','output_schema','program','args','timeout_seconds']);
for(const t of bundle.command_tools){assert.ok(Object.keys(t).every(k=>keys.has(k)),t.name);assert.ok(t.timeout_seconds>=1&&t.timeout_seconds<=600);assert.deepEqual(t.args,['--tool',t.name]);}
await access(join(dest,bundle.skill),constants.R_OK);
const binary=join(dest,bundle.local_service.program);await access(binary,constants.X_OK);
const pageRoot=join(dest,'page');const html=await readFile(join(dest,bundle.pages[0].entry),'utf8');
const assets=[...html.matchAll(/(?:src|href)="([^"]+)"/g)].map(m=>m[1]);
assert.ok(assets.some(p=>p.endsWith('.css')),'independent CSS required');
assert.ok(assets.some(p=>p.endsWith('.js')));
for(const asset of assets){assert.ok(asset.startsWith('./'),asset);const path=await realpath(resolve(pageRoot,asset));assert.ok(path.startsWith(pageRoot+sep));await access(path);if(asset.endsWith('.css'))assert.doesNotMatch(await readFile(path,'utf8'),/@import\s|url\(\s*['"]?(?:https?:)?\/\//i);}
const f=await fixture();let backend;
const local={sqlite_path:'verify.sqlite'};
try {
  backend=await start(binary,f);
  const call=async(path,data,method=data?'POST':'GET')=>{const r=await fetch(backend.origin+'/api/'+path,{method,...(data?{headers:{'Content-Type':'application/json'},body:JSON.stringify(data)}:{})});assert.ok(r.ok,await r.clone().text());return r.json();};
  assert.deepEqual(await call('health'),{ok:true,engine:'rust-db',drivers:['sqlite','mysql','postgres']});
  rpc(binary,f,'execute',{...local,sql:'CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, qty INTEGER NOT NULL)'});
  rpc(binary,f,'execute',{...local,sql:'INSERT INTO products VALUES (1,?,?),(2,?,?)',params:['青茶',5,'红茶',9]});
  assert.equal(rpc(binary,f,'list_connections',{}).workspace_sqlite[0].path,local.sqlite_path);
  const saved=rpc(binary,f,'save_connection',{name:'verify',driver:'sqlite',env:'dev',file_path:local.sqlite_path,test_connection:true});assert.equal(saved.test.ok,true);assert.equal(saved.connection.has_password,false);assert.equal('password' in saved.connection,false);
  const schema=rpc(binary,f,'inspect_schema',{...local,schema:'main',table:'products',include_ddl:true});assert.deepEqual(schema.primary_key,['id']);assert.match(schema.ddl,/CREATE TABLE/);
  const p=rpc(binary,f,'query',{...local,sql:'SELECT * FROM products ORDER BY id',limit:1});assert.deepEqual(p.rows,[[1,'青茶',5]]);assert.equal(p.next_offset,1);
  assert.match(JSON.stringify(rpc(binary,f,'explain',{...local,sql:'SELECT * FROM products WHERE id=1'}).rows),/PRIMARY KEY/i);
  const out=rpc(binary,f,'export_xlsx',{...local,sql:'SELECT * FROM products ORDER BY id',output_path:'verified.xlsx'});assert.equal(out.rows,2);
  assert.equal((await readFile(join(f.root,'verified.xlsx'))).subarray(0,2).toString(),'PK');
  const imported=rpc(binary,f,'import_xlsx',{...local,xlsx_path:'verified.xlsx',table_name:'restored',mode:'create_and_insert'});assert.equal(imported.rows_inserted,2);
  assert.deepEqual((await call('query',{...local,sql:'SELECT * FROM restored ORDER BY id'})).rows,[[1,'青茶',5],[2,'红茶',9]]);
  const snapshot=await fetch(backend.origin+'/api/sqlite/attachment',{method:'POST',headers:{'Content-Type':'application/octet-stream'},body:await readFile(join(f.root,local.sqlite_path))});assert.equal(snapshot.status,200);
  const handle=await snapshot.json();assert.deepEqual((await call('query',{...handle,sql:'SELECT qty FROM products ORDER BY id'})).rows,[[5],[9]]);
  const denied=await fetch(backend.origin+'/api/execute',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...handle,sql:'DELETE FROM products'})});assert.equal(denied.status,400);assert.match((await denied.json()).error,/read_only/);
  assert.equal((await call('connections/'+saved.connection.id,undefined,'DELETE')).deleted,saved.connection.id);
  const focused=rpc(binary,f,'focus_view',{table:'products',column:'name'});assert.equal(focused.focused,true);assert.equal(focused.column,'name');
  console.log('DB bundle verified: 9 strict tools, '+assets.length+' offline assets, HTTP health + attachment, stdio SQLite → XLSX → SQLite, db-server '+(await stat(binary)).size+' bytes');
}finally{await backend?.stop();await f.clean();}
