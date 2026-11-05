import http from 'node:http';import {readFile} from 'node:fs/promises';import path from 'node:path';import {writeFileSync,mkdtempSync} from 'node:fs';import os from 'node:os';
import {chromium,webkit} from 'playwright-core';
const HERE=path.dirname(new URL(import.meta.url).pathname);const ROOT=path.join(HERE,'www');
// The chart libraries come from the lockfile, not from copies in the tree. `--caps-only` stops after the capability probe.
const LIBS={'uPlot.iife.min.js':'uplot/dist/uPlot.iife.min.js','uPlot.min.css':'uplot/dist/uPlot.min.css','echarts.min.js':'echarts/dist/echarts.min.js'};
const CAPS_ONLY=process.argv.includes('--caps-only');const OUT=mkdtempSync(path.join(os.tmpdir(),'debrief-drawing-'));
const STRICT="default-src 'self' ipc: http://ipc.localhost; style-src 'self'; font-src 'self'; img-src 'self' data:; media-src 'self' media: http://media.localhost; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'self'; form-action 'none'; frame-ancestors 'none'";
const T={'.html':'text/html','.js':'text/javascript','.css':'text/css'};
const srv=http.createServer(async(q,s)=>{try{const u=q.url==='/'?'/index.html':q.url.split('?')[0];const n=path.basename(u);const b=await readFile(LIBS[n]?path.join(HERE,'node_modules',LIBS[n]):path.join(ROOT,n));s.writeHead(200,{'content-type':T[path.extname(u)]||'text/plain','content-security-policy':STRICT});s.end(b)}catch{s.writeHead(404);s.end()}});
await new Promise(r=>srv.listen(0,'127.0.0.1',r));const port=srv.address().port;
const TARGETS=[{name:'webkit',t:webkit,o:{}},{name:'chromium-gpu',t:chromium,o:{channel:'chromium',args:['--js-flags=--expose-gc','--enable-precise-memory-info']}},{name:'chromium-headless-shell',t:chromium,o:{args:['--js-flags=--expose-gc','--enable-precise-memory-info']}}];
const only='all';const browsers=[];setTimeout(async()=>{console.log('WATCHDOG 18 min: closing');for(const b of browsers)await b.close().catch(()=>{});process.exit(3)},18*60*1000).unref();const out={};const started=Date.now();
const log=(...a)=>console.log(new Date().toISOString().slice(11,19),...a);
for(const tg of TARGETS){log('launch',tg.name);let b;try{b=await tg.t.launch({headless:true,...tg.o});browsers.push(b);
 const ctx=await b.newContext({viewport:{width:1600,height:900},deviceScaleFactor:2});const p=await ctx.newPage();p.setDefaultTimeout(60000);p.on('pageerror',e=>log('pageerror',e.message));
 await p.goto(`http://127.0.0.1:${port}/`);const res=out[tg.name]={version:b.version()};
 res.caps=await p.evaluate(()=>CAPS());log(JSON.stringify(res.caps));if(CAPS_ONLY){await ctx.close();continue}
 res.base=await p.evaluate(()=>BASE());log('base',JSON.stringify(res.base));
 res.decode=await p.evaluate(()=>DECODE());log(JSON.stringify(res.decode));
 const lad=await p.evaluate(()=>LADDER);res.ladder={};
 for(const [k,fn] of [['grouped','c'],['path','c'],['each','c'],['text','c'],['worker','w'],['gl','g'],['dom','d'],['svg','s']]){res.ladder[k]=[];
  if(k==='gl'&&!res.caps.gl2){res.ladder[k]='no webgl2';log(k,'none');continue}
  if(k==='worker'&&!res.caps.transfer){continue}
  for(const n of lad[k]){const r=await p.evaluate(([k,fn,n])=>fn==='c'?CANVAS(k,n):fn==='w'?WORKER(n):fn==='g'?GL(n):fn==='d'?DOMN(n):SVGL(n),[k,fn,n]);r.n=n;res.ladder[k].push(r);log(k,n,JSON.stringify(r));
   if(r.none||r.rafP95>50||r.jsP50>50)break}}
 res.charts=[];for(const lib of ['uplot','echarts-lttb','echarts'])for(const n of [1e5,1e6]){const r=await p.evaluate(([l,n])=>CHART(l,n),[lib,n]);r.lib=lib;res.charts.push(r);log(lib,JSON.stringify(r))}
 res.memoryNote='';await ctx.close()}catch(e){log('FAILED',tg.name,e.message.split('\n')[0]);out[tg.name]={...(out[tg.name]||{}),error:e.message.split('\n')[0]}}finally{if(b)await b.close();writeFileSync(path.join(OUT,`partial-${tg.name}.json`),JSON.stringify(out[tg.name]||{},null,1))}}
srv.close();writeFileSync(path.join(OUT,'results.json'),JSON.stringify(out,null,1));log('done',(Date.now()-started)/1000,'s, results in',OUT);process.exit(0);
