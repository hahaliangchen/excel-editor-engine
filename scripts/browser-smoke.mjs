import assert from 'node:assert/strict';
import {mkdir,readFile} from 'node:fs/promises';
import {join} from 'node:path';
import {chromium} from 'playwright';

const url=process.env.STUDIO_URL || 'http://127.0.0.1:4183/';
const output=process.env.BROWSER_ARTIFACTS || 'artifacts/browser';
await mkdir(output,{recursive:true});
const browser=await chromium.launch({headless:true,channel:process.env.PLAYWRIGHT_CHANNEL || 'chrome'});
const context=await browser.newContext({viewport:{width:1440,height:900},colorScheme:'light',acceptDownloads:true,permissions:['clipboard-read','clipboard-write']});
const page=await context.newPage();
const errors=[];
const foreign=new Set();
const badResponses=[];
const origins=new Set([new URL(url).origin,'http://127.0.0.1:8083']);
page.on('pageerror',error=>errors.push(error.message));
page.on('console',message=>{if(message.type()==='error')errors.push(message.text());});
page.on('request',request=>{const u=new URL(request.url());if(['http:','https:'].includes(u.protocol)&&!origins.has(u.origin))foreign.add(request.url());});
page.on('response',response=>{if(response.status()>=400)badResponses.push(response.status()+' '+response.url());});
page.on('requestfailed',request=>errors.push(request.url()+' '+request.failure()?.errorText));
await page.addInitScript(()=>{
  new MutationObserver(()=>{
    if (window.__excelFirstRender || !document.querySelector('.studio .grid')) return;
    window.__excelFirstRender={css:[...document.styleSheets].some(s=>s.href?.includes('/assets/')&&s.href.endsWith('.css')),theme:document.documentElement.dataset.theme};
  }).observe(document,{childList:true,subtree:true});
});
const cell=a=>page.locator('[data-address="'+a+'"]');
const tab=name=>page.getByRole('tab',{name,exact:true});
const btn=name=>page.getByRole('button',{name,exact:true});
const fx=page.getByRole('textbox',{name:'公式栏'});
const waitCell=(a,text)=>page.waitForFunction(({a,text})=>document.querySelector('[data-address="'+a+'"]')?.textContent===text,{a,text});
const waitStatus=()=>page.waitForFunction(()=>document.querySelector('.engine-status')?.textContent==='就绪');
const assertValue=async(locator,text)=>assert.equal(await locator.inputValue(),text);
const stats=async expected=>{
  for(const [key,value] of Object.entries(expected))assert.equal(await page.locator('[data-stat="'+key+'"]').textContent(),value,key);
};
async function drag(from,to){
  const handle=await page.waitForFunction(addresses=>{
    const points=addresses.map(a=>document.querySelector('[data-address="'+a+'"]')?.getBoundingClientRect());
    return points.every(p=>p && p.width && p.height) ? points.map(p=>[p.x+p.width/2,p.y+p.height/2]) : false;
  },[from,to]);
  const [a,b]=await handle.jsonValue();
  await page.mouse.move(...a);await page.mouse.down();await page.mouse.move(...b,{steps:8});await page.mouse.up();
}
async function jump(a){const name=page.getByRole('textbox',{name:'名称框'});await name.fill(a);await name.press('Enter');await cell(a).waitFor();}
let checks=0;
try{
  await page.goto(url,{waitUntil:'networkidle'});
  assert.equal(await page.title(),'Excel 工作台');
  assert.deepEqual(await page.evaluate(()=>window.__excelFirstRender),{css:true,theme:'light'});checks++;
  await Promise.all([page.waitForResponse(r=>r.url().endsWith('/api/xlsx/sample')&&r.ok()),btn('载入示例工作簿').click()]);
  await waitCell('D4','¥128,000.00');await waitCell('G4','43.8%');await waitCell('D13','¥975,000.00');
  assert.equal(await cell('A1').getAttribute('aria-colspan'),'8');assert.equal(await tab('部门汇总').count(),1);checks++;

  await cell('D4').dblclick();
  const inline=page.getByRole('textbox',{name:'编辑单元格'});await assertValue(inline,'128000');
  await inline.fill('148000');await assertValue(fx,'148000');await inline.press('Enter');
  await waitCell('F4','¥76,000.00');await waitCell('D13','¥995,000.00');await waitStatus();checks++;
  await tab('部门汇总').click();await waitCell('B4','¥478,000.00');await waitCell('B8','¥995,000.00');
  await cell('B4').click();assert.match(await fx.inputValue(),/^=SUMIF\(/);checks++;

  await btn('撤销 (Ctrl+Z)').click();await waitCell('D4','¥128,000.00');
  await tab('部门汇总').click();await waitCell('B4','¥458,000.00');
  await btn('重做 (Ctrl+Y)').click();await waitCell('B4','¥478,000.00');checks++;

  await tab('季度营收明细').click();await waitCell('D4','¥148,000.00');await drag('D4','D12');
  await page.waitForFunction(()=>document.querySelector('.range-label')?.textContent==='D4:D12');
  await stats({count:'9',sum:'995,000',average:'110,555.556',min:'64,000',max:'176,000'});
  await page.screenshot({path:join(output,'excel-light.png')});checks++;
  await btn('切换为深色').click();assert.equal(await page.locator('html').getAttribute('data-theme'),'dark');
  await page.screenshot({path:join(output,'excel-dark.png')});checks++;

  const [download]=await Promise.all([page.waitForEvent('download'),btn('导出 .xlsx').click()]);
  const exported=join(output,'excel-roundtrip.xlsx');await download.saveAs(exported);
  assert.equal((await readFile(exported)).subarray(0,4).toString('hex'),'504b0304');
  await page.locator('#xlsx-file').setInputFiles(exported);await waitStatus();await waitCell('D4','¥148,000.00');
  assert.equal(await cell('A1').getAttribute('aria-colspan'),'8');await waitCell('G4','51.4%');await waitCell('F4','¥76,000.00');
  await tab('部门汇总').click();await waitCell('B4','¥478,000.00');await waitCell('B8','¥995,000.00');
  await cell('B4').click();assert.match(await fx.inputValue(),/^=SUMIF\(/);checks++;

  await tab('季度营收明细').click();await jump('J4');
  await page.evaluate(()=>navigator.clipboard.writeText('11\t22\r\n33\t44'));
  await page.keyboard.press('Control+v');await waitCell('K5','44');await stats({count:'4',sum:'110',average:'27.5'});checks++;
  await jump('J7');await fx.fill('=SUM(J4:K5)');await fx.press('Enter');await waitCell('J7','110');
  await cell('J7').click();await assertValue(fx,'=SUM(J4:K5)');checks++;

  await jump('D4');const widthBefore=(await cell('D4').boundingBox()).width;
  const grip=await page.locator('[data-resize="4"]').boundingBox();assert(grip);
  await page.mouse.move(grip.x+grip.width/2,grip.y+grip.height/2);await page.mouse.down();await page.mouse.move(grip.x+grip.width/2+32,grip.y+grip.height/2,{steps:5});await page.mouse.up();await waitStatus();
  await page.waitForFunction(w=>document.querySelector('[data-address="D4"]').getBoundingClientRect().width>w+20,widthBefore);
  await btn('加粗 (Ctrl+B)').click();await waitStatus();await page.waitForFunction(()=>document.querySelector('[data-address="D4"]')?.classList.contains('bold'));checks++;

  await jump('XFD1048576');await page.waitForFunction(()=>document.querySelector('[data-address="XFD1048576"]')?.classList.contains('active'));
  assert((await page.getByRole('gridcell').count())<1400,'large viewport is virtualized');
  const body=await page.locator('.sheet-body').boundingBox(),head=await page.locator('.column-heads').boundingBox();assert.equal(body.y,head.y);checks++;

  await page.setViewportSize({width:420,height:820});await tab('部门汇总').click();await waitCell('B4','¥478,000.00');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth===window.innerWidth));
  await page.screenshot({path:join(output,'excel-mobile-dark.png')});checks++;
  assert.deepEqual(errors,[],'Console / runtime errors');assert.deepEqual(badResponses,[],'HTTP error responses');assert.deepEqual([...foreign],[],'External resources');checks++;
  console.log(checks+' browser checks passed; 0 console errors, 0 failed HTTP requests, 0 external origins.');
  console.log('Cross-sheet 458,000 → 478,000; aggregate 995,000 / 110,555.556; screenshots and XLSX in '+output+'.');
} finally {await browser.close();}
