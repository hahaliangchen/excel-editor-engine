import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile,mkdir,stat} from 'node:fs/promises';
import {join,resolve,sep,extname} from 'node:path';
import {once} from 'node:events';
import {chromium} from 'playwright';
import {fixture,rpc,start} from './quickagent-db/runtime.mjs';

const output=resolve(process.env.BROWSER_ARTIFACTS||'artifacts/browser');
const executable=resolve(process.env.DB_STUDIO_BINARY||'rust-db/target/debug/db-server');
const assets=resolve(process.env.DB_STUDIO_ASSETS||'packages/db-studio/dist');
await mkdir(output,{recursive:true});
const f=await fixture();let backend,front,browser;
const pageRoot='/agent/skill-packages/db/pages/studio/';
const apiRoot='/agent/skill-packages/db/local-api';
const types={'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.svg':'image/svg+xml'};
let checks=0;
async function frontServer(){
  const server=createServer(async(req,res)=>{
    try{
      const url=new URL(req.url,'http://localhost');
      if(url.pathname.startsWith(apiRoot+'/api/')){
        const body=[];for await(const p of req)body.push(p);
        const back=await fetch(backend.origin+url.pathname.slice(apiRoot.length)+url.search,{method:req.method,headers:{...(req.headers['content-type']?{'Content-Type':req.headers['content-type']}:{})},...(!['GET','HEAD'].includes(req.method)?{body:Buffer.concat(body)}:{})});
        res.writeHead(back.status,Object.fromEntries(back.headers.entries().filter(([k])=>!['transfer-encoding','connection','content-length'].includes(k))));res.end(Buffer.from(await back.arrayBuffer()));return;
      }
      if(url.pathname.startsWith('/agent/tasks/db/attachments/')) {res.setHeader('Content-Type','application/octet-stream');res.end(await readFile(join(f.root,'light.sqlite')));return;}
      if(url.pathname==='/favicon.ico'){res.writeHead(204).end();return;}
      if(!url.pathname.startsWith(pageRoot)){res.writeHead(404).end();return;}
      const path=resolve(assets,url.pathname.slice(pageRoot.length)==='index'?'index.html':url.pathname.slice(pageRoot.length));
      if(!path.startsWith(assets+sep)){res.writeHead(404).end();return;}
      res.setHeader('Content-Type',types[extname(path)]||'application/octet-stream');res.end(await readFile(path));
    }catch(e){res.writeHead(500).end(String(e));}
  });
  server.listen(0,'127.0.0.1');await once(server,'listening');return server;
}
async function checkTheme(theme,origin){
  const sqlite_path=theme+'.sqlite';const target={sqlite_path};
  rpc(executable,f,'execute',{...target,sql:'CREATE TABLE inventory (id INTEGER PRIMARY KEY, sku TEXT NOT NULL, name TEXT NOT NULL, quantity INTEGER, price REAL, category TEXT, location TEXT, notes TEXT)'});
  rpc(executable,f,'execute',{...target,sql:'CREATE INDEX index_inventory_sku ON inventory(sku)'});
  rpc(executable,f,'execute',{...target,sql:'CREATE VIEW low_stock AS SELECT * FROM inventory WHERE quantity<12'});
  const names=['手冲咖啡壶','滤纸 100 张','磨豆机','温控水壶','玻璃分享壶','称重电子秤','清洁刷','保温杯'];
  const values=names.map((name,i)=>[i+1,'DB-'+(1001+i),name,[24,140,12,31,50,22,7,39][i], [129,29.5,249,320,89,119,38,98][i],['器具','耗材','器具','电器'][i%4],i%2?'上海仓':'杭州仓',i%3===1?'适用于日常备货':null]);
  rpc(executable,f,'execute',{...target,sql:'INSERT INTO inventory VALUES '+values.map(()=>'(?,?,?,?,?,?,?,?)').join(','),params:values.flat()});
  const context=await browser.newContext({viewport:{width:1440,height:900},colorScheme:theme,acceptDownloads:true});
  const page=await context.newPage();page.setDefaultTimeout(6000);
  const errors=[],foreign=[],failed=[];
  page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
  page.on('request',r=>{const u=new URL(r.url());if(['http:','https:'].includes(u.protocol)&&u.origin!==origin)foreign.push(r.url());});
  page.on('response',r=>{if(r.status()>=400)failed.push(r.status()+' '+r.url());});
  page.on('requestfailed',r=>errors.push(r.url()+' '+r.failure()?.errorText));
  await page.addInitScript(()=>{new MutationObserver(()=>{if(window.__dbInitial||!document.querySelector('.db-shell'))return;window.__dbInitial={theme:document.documentElement.dataset.theme,css:[...document.styleSheets].some(s=>s.href?.includes('/assets/')&&s.href.endsWith('.css'))};}).observe(document,{childList:true,subtree:true});});
  const btn=(name)=>page.getByRole('button',{name,exact:true});const cell=a=>page.locator('.grid-cell[data-address="'+a+'"]');
  const editor=page.getByRole('textbox',{name:'SQL 编辑器'});
  const waitCell=(address,value)=>page.waitForFunction(({address,value})=>document.querySelector('.grid-cell[data-address="'+address+'"]')?.textContent===value,{address,value});
  try{
    const url=origin+pageRoot+'index?'+new URLSearchParams({workspace_root:f.root,theme});
    await page.goto(url,{waitUntil:'networkidle'});
    assert.deepEqual(await page.evaluate(()=>window.__dbInitial),{theme,css:true});checks++;
    await btn(sqlite_path).click();await btn('inventory').waitFor();assert.equal(await btn('low_stock').count(),1);checks++;
    await btn('inventory').click();await waitCell('D2','24');assert.equal(await editor.inputValue(),'SELECT * FROM "main"."inventory"');checks++;
    await editor.fill('SELECT * FROM "main"."inventory" ORDER BY id');await editor.press('Control+Enter');await page.waitForFunction(()=>document.querySelector('.db-job')?.textContent?.startsWith('查询完成'));
    assert.equal(await cell('C2').textContent(),'手冲咖啡壶');checks++;
    await cell('D2').dblclick();const inline=page.getByRole('textbox',{name:'编辑单元格'});await inline.fill('36');await inline.press('Enter');await waitCell('D2','36');assert.match(await cell('D2').getAttribute('class'),/db-dirty/);checks++;
    await btn('预览 SQL 并提交').click();const dialog=page.getByRole('dialog',{name:'确认数据库写入'});assert.match(await dialog.locator('pre').textContent(),/UPDATE "main"\."inventory" SET "quantity" = \? WHERE "id" = \? AND "quantity" = \?/);
    await btn('确认提交').click();await page.waitForFunction(()=>document.querySelector('.db-job')?.textContent?.startsWith('已提交'));
    assert.deepEqual(rpc(executable,f,'query',{...target,sql:'SELECT quantity FROM inventory WHERE id=1'}).rows,[[36]]);checks++;
    await btn('表结构 / DDL').click();await page.getByRole('heading',{name:'main.inventory'}).waitFor();assert.match(await page.locator('.db-rich').textContent(),/CREATE TABLE inventory/);checks++;
    await btn('执行计划').click();await page.getByRole('heading',{name:'执行计划'}).waitFor();assert.match(await page.locator('.db-rich').textContent(),/SCAN/i);checks++;
    await btn('结果数据').click();await page.getByRole('searchbox',{name:'筛选当前页'}).fill('磨豆机');await waitCell('C2','磨豆机');await page.getByRole('searchbox',{name:'筛选当前页'}).fill('');await waitCell('C2','手冲咖啡壶');checks++;
    await btn('导出 Excel (.xlsx)').click();const dl=page.waitForEvent('download');await btn('下载 .xlsx').click();const book=await dl;const file='db-'+theme+'-roundtrip.xlsx';await book.saveAs(join(output,file));await book.saveAs(join(f.root,file));
    const restored=rpc(executable,f,'import_xlsx',{...target,xlsx_path:file,table_name:'restored',mode:'create_and_insert'});assert.equal(restored.rows_inserted,8);assert.deepEqual(rpc(executable,f,'query',{...target,sql:'SELECT quantity FROM restored WHERE id=1'}).rows,[[36]]);checks++;
    await page.screenshot({path:join(output,'db-'+theme+'.png'),fullPage:true});checks++;
    assert.deepEqual(errors,[]);assert.deepEqual(failed,[]);assert.deepEqual(foreign,[]);checks++;
    console.log(theme+': SQLite schema, Ctrl+Enter, inline update 24 → 36, DDL, EXPLAIN, filter, XLSX round trip (8 rows), screenshot; 0 console errors, 0 HTTP failures, 0 external requests');
  }catch(e){await page.screenshot({path:join(output,'db-'+theme+'-failure.png')});console.error({errors,failed,foreign,status:await page.locator('.db-job').textContent().catch(()=>null)});throw e;}finally{await context.close();}
}
try{
  await stat(executable);backend=await start(executable,f);front=await frontServer();const origin='http://127.0.0.1:'+front.address().port;
  browser=await chromium.launch({headless:true,channel:process.env.PLAYWRIGHT_CHANNEL||'chrome'});
  await checkTheme('light',origin);await checkTheme('dark',origin);
  const attachment=await browser.newPage({viewport:{width:390,height:844}});
  try{
    await attachment.goto(origin+pageRoot+'index?'+new URLSearchParams({theme:'dark',workspace_root:f.root,file:'/agent/tasks/db/attachments/fixture.sqlite',name:'附件 SQLite'}));
    await attachment.getByRole('tab',{name:'inventory'}).waitFor();
    assert.equal(await attachment.getByRole('button',{name:'执行写入',exact:true}).isDisabled(),true);
    assert.equal(await attachment.evaluate(()=>document.documentElement.scrollWidth===innerWidth),true);
    const viewport=attachment.locator('.db-grid-wrap .grid-viewport');
    await viewport.focus();
    for(let i=0;i<7;i++)await viewport.press('ArrowRight');
    await attachment.waitForFunction(()=>document.querySelector('.db-grid-wrap .grid-viewport')?.scrollLeft>0&&document.querySelector('[data-address="H1"]')?.classList.contains('active'));
    await viewport.press('Home');
    await attachment.screenshot({path:join(output,'db-mobile-attachment.png')});checks++;
  }finally{await attachment.close();}
  console.log('DB Playwright acceptance: '+checks+' checks passed');
}finally{await browser?.close();if(front)await new Promise(r=>front.close(r));await backend?.stop();await f.clean();}
