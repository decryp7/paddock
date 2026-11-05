const stage=document.getElementById('stage');const viol=[];
document.addEventListener('securitypolicyviolation',e=>viol.push(e.violatedDirective+' '+(e.blockedURI||'')));
const nextFrame=()=>new Promise(r=>requestAnimationFrame(()=>r()));
const FR=60,MAXMS=2500,LADDER={each:[1e3,5e3,1e4,2.5e4,5e4,1e5,2e5,4e5],grouped:[1e3,1e4,2.5e4,5e4,1e5,2e5,4e5,8e5],path:[1e3,1e4,2.5e4,5e4,1e5,2e5,4e5,8e5],text:[50,100,200,400,800,1600,3200,6400],gl:[1e4,1e5,2e5,4e5,8e5,16e5,32e5],worker:[1e3,1e4,2.5e4,5e4,1e5,2e5,4e5,8e5],dom:[100,500,1000,2000,5000,1e4,2e4],svg:[5,10,20,40,80]};
function cnv(opts){stage.innerHTML='';const c=document.createElement('canvas');const d=devicePixelRatio||1;c.width=1600*d;c.height=900*d;c.style.width='1600px';c.style.height='900px';stage.append(c);return c}
window.CAPS=async()=>{const o={ua:navigator.userAgent,dpr:devicePixelRatio};
 o.offscreen=typeof OffscreenCanvas==='function';o.transfer=typeof HTMLCanvasElement.prototype.transferControlToOffscreen==='function';
 o.dpcb=typeof ResizeObserverEntry!=='undefined'&&'devicePixelContentBoxSize' in ResizeObserverEntry.prototype;
 o.gpu=!!navigator.gpu;if(navigator.gpu){try{const a=await navigator.gpu.requestAdapter();o.gpuAdapter=!!a}catch(e){o.gpuAdapter='throw'}}
 const g=document.createElement('canvas').getContext('webgl2');o.gl2=!!g;
 if(g){const e=g.getExtension('WEBGL_debug_renderer_info');o.glRenderer=e?g.getParameter(e.UNMASKED_RENDERER_WEBGL):g.getParameter(g.RENDERER)}
 o.worker=await new Promise(r=>{try{const w=new Worker('worker.js');w.onmessage=e=>{r(e.data);w.terminate()};w.onerror=e=>r('error '+e.message);w.postMessage({kind:'caps'})}catch(e){r('throw '+e.name)}});
 o.blobWorker=await new Promise(r=>{try{const u=URL.createObjectURL(new Blob(['postMessage(1)'],{type:'text/javascript'}));const w=new Worker(u);const t=setTimeout(()=>r('no message (blocked)'),1500);w.onmessage=()=>{clearTimeout(t);r('ran')};w.onerror=()=>{clearTimeout(t);r('blocked')}}catch(e){r('throw '+e.name)}});
 try{const m=new Uint8Array([0,97,115,109,1,0,0,0]);await WebAssembly.instantiate(m);o.wasm='ran'}catch(e){o.wasm='blocked '+e.name}
 try{new Function('return 1')();o.eval='ran'}catch(e){o.eval='blocked'}
 const el=document.createElement('div');el.style.width='3px';stage.append(el);o.cssomStyle=getComputedStyle(el).width;el.setAttribute('style','width:9px');o.attrStyle=getComputedStyle(el).width;el.remove();
 await new Promise(r=>setTimeout(r,100));o.violations=[...viol];return o};
window.BASE=async()=>{const r=await loop(requestAnimationFrame.bind(window),()=>{},FR,MAXMS);return r};
window.CANVAS=async(mode,n)=>{const c=cnv();const d=devicePixelRatio||1;const x=c.getContext('2d',{alpha:false});x.setTransform(d,0,0,d,0,0);const R=mkRects(n);
 const r=await loop(requestAnimationFrame.bind(window),f=>{x.fillStyle='#fff';x.fillRect(0,0,1600,900);draw2d(x,R,n,mode,f*3)},FR,MAXMS);
 r.check=Array.from(x.getImageData(0,0,1600,900).data).some((v,i)=>i%4==0&&v!==255&&v!==0);
 const t=performance.now();x.fillStyle='#fff';x.fillRect(0,0,1600,900);draw2d(x,R,n,mode,7);x.getImageData(0,0,1,1);r.flushMs=+(performance.now()-t).toFixed(1);return r};
window.WORKER=async(n)=>{const c=cnv();const off=c.transferControlToOffscreen();const w=new Worker('worker.js');const d=devicePixelRatio||1;
 const once=(m,t)=>new Promise(res=>{w.onmessage=e=>res(e.data);w.postMessage(m,t||[])});await once({kind:'init',canvas:off,dpr:d},[off]);const r=await once({kind:'run',n,frames:FR,maxMs:MAXMS});w.terminate();return r};
window.GL=async(n)=>{stage.innerHTML='';const c=cnv();const g=c.getContext('webgl2',{alpha:false,antialias:false});if(!g)return {none:true};const d=devicePixelRatio||1;const R=mkRects(n);const u=gl2(g,R,n);g.viewport(0,0,1600*d,900*d);
 const fr=(f)=>{g.clearColor(1,1,1,1);g.clear(g.COLOR_BUFFER_BIT);g.uniform1f(u,f*3);g.drawArraysInstanced(g.TRIANGLE_STRIP,0,4,n)};
 const r=await loop(requestAnimationFrame.bind(window),fr,FR,MAXMS);const t=performance.now();fr(9);const px=new Uint8Array(4);g.readPixels(0,0,1,1,g.RGBA,g.UNSIGNED_BYTE,px);r.flushMs=+(performance.now()-t).toFixed(1);r.err=g.getError();return r};
window.DOMN=async(n)=>{stage.innerHTML='';const R=mkRects(n);const els=[];const fr=document.createDocumentFragment();for(let i=0;i<n;i++){const e=document.createElement('div');e.className='d';e.style.height=R.h[i]+'px';e.style.background=COLS[R.c[i]];els.push(e);fr.append(e)}stage.append(fr);
 const r=await loop(requestAnimationFrame.bind(window),f=>{for(let i=0;i<n;i++)els[i].style.transform=`translate(${(R.x[i]+f*3)%1600}px,${R.y[i]}px)`},FR,MAXMS);stage.innerHTML='';return r};
// The current lanes shape: an SVG per lane with a bar path of 2000 buckets, rebuilt on each change (histogram.ts, timeline-lanes.ts).
window.SVGL=async(lanes)=>{stage.innerHTML='';const NS='http://www.w3.org/2000/svg';const ps=[];for(let l=0;l<lanes;l++){const s=document.createElementNS(NS,'svg');s.setAttribute('viewBox','0 0 2000 100');s.setAttribute('preserveAspectRatio','none');s.style.cssText='';s.setAttribute('width','1600');s.setAttribute('height','20');s.style.display='block';const p=document.createElementNS(NS,'path');p.setAttribute('fill',COLS[l%3]);s.append(p);stage.append(s);ps.push(p)}
 const hs=new Float32Array(2000).map((_,i)=>5+((i*7919)%90));
 const r=await loop(requestAnimationFrame.bind(window),f=>{for(let l=0;l<lanes;l++){const parts=[];for(let x=0;x<2000;x++){const h=hs[(x+f*3+l*11)%2000];parts.push('M'+(x+.12).toFixed(2)+' '+(100-h).toFixed(2)+'h0.76v'+h.toFixed(2)+'h-0.76z')}ps[l].setAttribute('d',parts.join(''))}},FR,MAXMS);stage.innerHTML='';return r};
// Decoding: JSON text vs a transferred buffer. Same numbers; shapes from api.types.ts.
window.DECODE=async()=>{const out={};const med=(f,k=7)=>{const v=[];for(let i=0;i<k;i++){const t=performance.now();f();v.push(performance.now()-t)}v.sort((a,b)=>a-b);return +v[k>>1].toFixed(2)};
 const lanes=[];for(let l=0;l<40;l++){const b=[];for(let i=0;i<2000;i++)b.push([0,(i*l)%9==0?1:0,(i*3)%5,1000+(i*l)%800,(i*l)%300,0]);lanes.push({key:'lane-'+l,buckets:b})}
 const lj=JSON.stringify(lanes);const lb=new Uint32Array(40*2000*6);for(let l=0;l<40;l++)for(let i=0;i<2000;i++)lb.set(lanes[l].buckets[i],(l*2000+i)*6);
 out.lanes={numbers:480000,jsonBytes:lj.length,binBytes:lb.byteLength,jsonParseMs:med(()=>JSON.parse(lj)),viewMs:med(()=>new Uint32Array(lb.buffer)),copyMs:med(()=>new Uint32Array(lb.buffer.slice(0)))};
 const rects=[];for(let i=0;i<200000;i++)rects.push({frame:i%5000,parent:i?(i-1)>>1:null,depth:i%64,x:i/200000,w:1/(1+i%500),value:1000+i,selfValue:i%100,matched:false});
 const rj=JSON.stringify(rects);out.flame={rects:200000,jsonBytes:rj.length,binBytes:200000*(4+4+2+8+8+8+8+1),jsonParseMs:med(()=>JSON.parse(rj),5)};
 const pts=[];for(let i=0;i<1e6;i++)pts.push(1730000000000+i*1000,(i%977)/3);const pj=JSON.stringify(pts);out.series={points:1e6,jsonBytes:pj.length,binBytes:16e6,jsonParseMs:med(()=>JSON.parse(pj),5)};
 const f=new Float64Array(1e6);for(let i=0;i<1e6;i++)f[i]=i*1000;
 const lo=(a,v)=>{let l=0,h=a.length;while(l<h){const m=(l+h)>>>1;if(a[m]<v)l=m+1;else h=m}return l};out.hitTest1M_ns=+(med(()=>{for(let i=0;i<1e5;i++)lo(f,(i*7919)%1e9)})*10).toFixed(0);
 if(performance.memory&&self.gc){const heap=()=>{gc();return performance.memory.usedJSHeapSize};let a=heap();let keep=JSON.parse(rj);let b=heap();out.flame.heapObjMB=+((b-a)/1e6).toFixed(1);keep=null;a=heap();keep=JSON.parse(lj);b=heap();out.lanes.heapObjMB=+((b-a)/1e6).toFixed(1);keep=null}
 return out};
window.CHART=async(lib,n)=>{stage.innerHTML='';const h=document.createElement('div');h.style.cssText='';h.id='ch';stage.append(h);h.style.width='1600px';h.style.height='400px';
 const x=new Float64Array(n),y=new Float64Array(n);for(let i=0;i<n;i++){x[i]=1.7e9+i;y[i]=Math.sin(i/500)*50+((i*7919)%100)/4}
 const t0=performance.now();let up;
 if(lib==='uplot'){up=new uPlot({width:1600,height:400,series:[{},{stroke:'#36c',width:1}]},[x,y],h)}
 else{const c=echarts.init(h,null,{renderer:'canvas',width:1600,height:400});const d=new Float64Array(n*2);for(let i=0;i<n;i++){d[2*i]=x[i];d[2*i+1]=y[i]}c.setOption({animation:false,xAxis:{type:'value',min:'dataMin',max:'dataMax'},yAxis:{type:'value'},series:[{type:'line',showSymbol:false,sampling:lib==='echarts-lttb'?'lttb':undefined,large:true,dimensions:['x','y'],data:d,encode:{x:0,y:1}}]});up={c}}
 const js=performance.now()-t0;await nextFrame();await nextFrame();const first=performance.now()-t0;
 const c2=h.querySelector('canvas');const s=performance.now();
 let re=0;void s;for(let i=0;i<5;i++){const a=performance.now();if(lib==='uplot')up.setScale('x',{min:x[0]+i*n/20,max:x[n-1]-i*n/20});else up.c.setOption({xAxis:{min:x[0]+i*n/20,max:x[n-1]-i*n/20}});await nextFrame();re+=performance.now()-a}
 const out={n,createMs:+js.toFixed(0),firstPaintMs:+first.toFixed(0),rezoomMs:+(re/5).toFixed(0),canvas:!!c2,violations:[...viol]};if(lib==='uplot')up.destroy();else up.c.dispose();return out};
window.LADDER=LADDER;
