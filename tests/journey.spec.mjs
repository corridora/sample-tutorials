import {test,expect} from '@playwright/test';
import fs from 'node:fs';
import http from 'node:http';
import {spawn} from 'node:child_process';
const hosted=!!process.env.SAMPLE_URL;
const base=process.env.SAMPLE_URL||'http://127.0.0.1:4106';
const token=process.env.DEMO_CONTROL_TOKEN||'local-test-control';
let child,sink;const captured=[];const sinkPort=14386;
test.beforeAll(async()=>{
 if(hosted)return;
 sink=http.createServer((req,res)=>{const b=[];req.on('data',x=>b.push(x));req.on('end',()=>{if(b.length)try{captured.push({path:req.url,body:JSON.parse(Buffer.concat(b))});}catch{}res.writeHead(200,{'Content-Type':'application/json','Access-Control-Allow-Origin':base,'Access-Control-Allow-Headers':'Content-Type,X-CORRIDORA-KEY','Access-Control-Allow-Methods':'POST,OPTIONS'});res.end('{}');});});await new Promise(r=>sink.listen(sinkPort,'127.0.0.1',r));
 child=spawn('./target/debug/local',[],{env:{...process.env,PORT:'4106',LOCAL_STATE_DIR:'.sample/browser-store',CORRIDORA_OTLP_ENDPOINT:`http://127.0.0.1:${sinkPort}`,CORRIDORA_INGEST_KEY:'mock-backend-key',CORRIDORA_RUM_KEY:process.env.RUM_BUNDLE_PATH?'mock-browser-key':'',DEMO_CONTROL_TOKEN:token,DEMO_DAILY_START_UTC:'00:00'},stdio:'pipe'});
 child.stderr.on('data',b=>process.stderr.write(b));
 for(let i=0;i<100;i++){try{if((await fetch(base+'/healthz')).ok)return;}catch{}await new Promise(r=>setTimeout(r,100));}throw new Error('local Rust server did not start');
});
test.afterAll(async()=>{child?.kill();await new Promise(r=>sink?sink.close(r):r());fs.mkdirSync('test-results',{recursive:true});fs.writeFileSync('test-results/otlp-capture.json',JSON.stringify(captured,null,2));});
async function phase(request,value,seconds=120){const r=await request.post(base+'/api/demo/scenario',{headers:{Authorization:`Bearer ${token}`},data:{phase:value,duration_seconds:seconds}});expect(r.status()).toBe(200);return r.json();}
function spans(){return captured.flatMap(c=>(c.body.resourceSpans||[]).flatMap(r=>(r.scopeSpans||[]).flatMap(s=>s.spans||[])));}
test('Astro search/progress and healthy → cache regression → control → expiry recovery',async({page,request})=>{
 if(process.env.RUM_BUNDLE_PATH){await page.route('https://rum.corridora.com/corridora-rum.js*',r=>r.fulfill({path:process.env.RUM_BUNDLE_PATH,contentType:'application/javascript'}));}
 await phase(request,'healthy');await page.goto(base);await page.waitForFunction(()=>window.SAMPLE_READY===true);await expect(page.getByRole('heading',{name:'Small lessons.'})).toBeVisible();
 await page.locator('#search').fill('edge');await page.locator('#search-button').click();await expect(page.locator('.lesson')).toHaveCount(1);await page.getByRole('button',{name:'Mark complete'}).click();await expect(page.locator('#progress')).not.toContainText('0 of 3');
 let started=Date.now();await page.locator('#origin').click();await expect(page.locator('#timing')).toContainText('baseline');const healthy=Date.now()-started;
 const expirySeconds=hosted?30:5;const override=await phase(request,'regression',expirySeconds);started=Date.now();await page.locator('#origin').click();await expect(page.locator('#timing')).toContainText('simulated cache bypass');const slow=Date.now()-started;expect(slow).toBeGreaterThan(1300);
 const trace=await page.evaluate(()=>window.SAMPLE_LAST_TRACE);expect(trace).toMatch(/^[0-9a-f]{32}$/);
 await page.locator('#control').click();await expect(page.locator('#origin-result')).toContainText('"affected": false');
 await new Promise(r=>setTimeout(r,(expirySeconds+1)*1000));const expired=await (await request.get(base+'/api/demo/status')).json();expect(expired.release).not.toBe(override.release);if(expired.phase==='regression')await phase(request,'recover');await page.locator('#origin').click();await expect(page.locator('#timing')).toContainText('baseline');
 await phase(request,'regression');await phase(request,'recover');await page.locator('#origin').click();await expect(page.locator('#timing')).toContainText('baseline');
 if(process.env.RUM_BUNDLE_PATH){await page.evaluate(async()=>{document.dispatchEvent(new Event('visibilitychange'));window.dispatchEvent(new Event('pagehide'));});await page.waitForTimeout(2500);const ss=spans();const server=ss.find(s=>s.traceId===trace&&Number(s.kind)===2);expect(server,'captured SERVER span').toBeTruthy();const client=ss.find(s=>s.traceId===trace&&s.spanId===server.parentSpanId&&Number(s.kind)===3);expect(client,'real RUM CLIENT parent of Rust SERVER').toBeTruthy();}
 const heartbeat=await request.post(base+'/api/demo/heartbeat',{headers:{Authorization:`Bearer ${token}`},data:{result:'passed',trace_id:trace}});expect(heartbeat.ok()).toBeTruthy();
 fs.mkdirSync('test-results',{recursive:true});fs.writeFileSync('test-results/evidence.json',JSON.stringify({trace_id:trace,healthy_ms:healthy,regression_ms:slow,browser_correlation:process.env.RUM_BUNDLE_PATH?'real-client-server-captured':'requires RUM_BUNDLE_PATH or Corridora UI',checked_at:new Date().toISOString()},null,2));
 await page.screenshot({path:'test-results/tutorials.png',fullPage:true});
});
