import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {access,mkdtemp,readFile,rm,stat} from 'node:fs/promises';
import {createServer} from 'node:net';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {setTimeout as sleep} from 'node:timers/promises';
import {manifest as expected} from './manifest.mjs';

const root=resolve(process.argv[2]??'.');
const manifest=JSON.parse(await readFile(join(root,'plugin.json'),'utf8'));
assert.deepEqual(manifest,expected);
const {entry}=manifest.pages[0];
const html=await readFile(join(root,entry),'utf8');
assert(!/(?:src|href)="(?:https?:)?\/\//i.test(html),'bundle contains an external asset');
for(const asset of html.matchAll(/(?:src|href)="\.\/([^"]+)"/g))await access(join(root,'page',asset[1]));
assert.match(html,/rel="stylesheet"[^>]*href="\.\/assets\/.+\.css"/);
assert.match(await readFile(join(root,'SKILL.md'),'utf8'),/^---\nname: excel\n/);
assert.equal(new Set(manifest.command_tools.map(t=>t.name)).size,9);
const bin=join(root,manifest.local_service.program);
const binInfo=await stat(bin);
assert(binInfo.isFile() && (binInfo.mode&0o111)!==0);

const workspace=await mkdtemp(join(tmpdir(),'excel-bundle-'));
let service;
try{
  const reservation=createServer();
  await new Promise((resolve,reject)=>reservation.listen(0,'127.0.0.1',resolve).once('error',reject));
  const {port}=reservation.address();await new Promise((resolve,reject)=>reservation.close(e=>e?reject(e):resolve()));
  service=spawn(bin,[],{env:{...process.env,BIND:'127.0.0.1:'+port},stdio:['ignore','ignore','pipe']});
  let logs='';service.stderr.on('data',b=>logs+=b);service.on('error',e=>logs+=e.message);
  const api='http://127.0.0.1:'+port+'/api/';
  let up=false;
  for(let i=0;i<50;i++){
    if(service.exitCode!==null)break;
    try{const r=await fetch(api+'health',{signal:AbortSignal.timeout(1000)});if(r.ok&&(await r.json()).engine==='rust-excel'){up=true;break;}}catch{ /* Service can start a moment after spawn. */ }
    await sleep(50);
  }
  assert(up,'bundled local_service failed: '+logs);
  const sample=await (await fetch(api+'xlsx/sample')).json();assert.equal(sample.sheets[1].cells.B4.value,458000);
  const rpc=(name,args)=>{
    const tool=manifest.command_tools.find(t=>t.name===name);
    const response=spawnSync(bin,tool.args,{input:JSON.stringify({jsonrpc:'2.0',id:1,method:'tools/call',params:{name,workspace_root:workspace,arguments:args}})+'\n',encoding:'utf8',timeout:10000,maxBuffer:200*1024});
    assert.equal(response.status,0,response.stderr||String(response.error));
    const result=JSON.parse(response.stdout);assert.equal(result.id,1);assert.equal(result.error,undefined,JSON.stringify(result.error));return result.result;
  };
  rpc('excel__create_xlsx',{path:'check.xlsx',workbook:{sheets:[{name:'Data',cells:{A1:{raw:'=2+3'}}}]}});
  assert.equal(rpc('excel__read_range',{path:'check.xlsx',sheet:'Data',range:'A1'}).items[0].value,5);
  console.log('Bundle verified: 8 manifest tools, standalone CSS, local HTTP with BIND, and stdio XLSX round trip. Binary: '+binInfo.size+' bytes.');
}finally{
  if(service && service.exitCode===null && service.pid){await new Promise(resolve=>{service.once('exit',resolve);service.kill();});}
  await rm(workspace,{recursive:true,force:true});
}
