import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp,mkdir,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';

export async function fixture() {
  const base=await mkdtemp(join(tmpdir(),'db-studio-'));
  const root=join(base,'workspace'), credentials=join(base,'credentials');await mkdir(root);
  return {root,env:{...process.env,DB_STUDIO_DATA_DIR:credentials,DB_STUDIO_WORKSPACE_ROOT:root},clean:()=>rm(base,{recursive:true,force:true})};
}
export function rpc(binary,f,suffix,args) {
  const name='db__'+suffix;
  const request={jsonrpc:'2.0',id:4,method:'tools/call',params:{name,workspace_root:f.root,arguments:args}};
  const child=spawnSync(binary,['--tool',name],{env:f.env,input:JSON.stringify(request)+'\n',encoding:'utf8',timeout:12000});
  assert.ifError(child.error);assert.equal(child.status,0,child.stderr);assert.ok(Buffer.byteLength(child.stdout)<=200*1024);
  const response=JSON.parse(child.stdout);assert.equal(response.id,4);assert.equal(response.jsonrpc,'2.0');assert.ok(!response.error,JSON.stringify(response.error));return response.result;
}
export async function start(binary,f) {
  const child=spawn(binary,[],{env:{...f.env,BIND:'127.0.0.1:0'},stdio:['ignore','pipe','pipe']});
  let stdout='',stderr='';child.stderr.on('data',data=>stderr+=data);
  let ended=false;child.once('exit',()=>ended=true);
  let timer;
  try{
    const origin=await new Promise((resolve,reject)=>{
      timer=setTimeout(()=>reject(new Error('db-server startup timed out: '+stdout+stderr)),12000);
      child.once('error',reject);child.once('exit',code=>reject(new Error('db-server exited '+code+': '+stderr)));
      child.stdout.on('data',chunk=>{stdout+=chunk;const m=/http:\/\/127\.0\.0\.1:\d+/.exec(stdout);if(m)resolve(m[0]);});
    });
    return {origin,stop:async()=>{if(!ended&&child.pid){const done=once(child,'exit');child.kill();await done;}}};
  }catch(e){child.kill();throw e;}finally{clearTimeout(timer);}
}
