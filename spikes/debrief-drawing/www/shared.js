// Deterministic geometry: n bars over 1600x900, 40 lanes of 22px, 3 colours. Fresh offsets every frame so nothing is cached.
self.COLS=['#3366cc','#cc7733','#cc3355'];
self.mkRects=function(n){const x=new Float32Array(n),y=new Float32Array(n),h=new Float32Array(n),c=new Uint8Array(n);
 let s=12345;const r=()=>(s=(s*1664525+1013904223)>>>0)/4294967296;
 for(let i=0;i<n;i++){x[i]=r()*1600;const l=Math.floor(r()*40);h[i]=2+r()*18;y[i]=l*22+(20-h[i]);c[i]=i%3}
 const order=[...Array(n).keys()].sort((a,b)=>c[a]-c[b]);
 return {x,y,h,c,order:Uint32Array.from(order)}};
self.draw2d=function(ctx,R,n,mode,off){
 if(mode==='each'){for(let i=0;i<n;i++){ctx.fillStyle=COLS[(i>>2)%3];ctx.fillRect((R.x[i]+off)%1600,R.y[i],1.5,R.h[i])}}
 else if(mode==='grouped'){let cur=-1;for(let k=0;k<n;k++){const i=R.order[k];if(R.c[i]!==cur){cur=R.c[i];ctx.fillStyle=COLS[cur]}ctx.fillRect((R.x[i]+off)%1600,R.y[i],1.5,R.h[i])}}
 else if(mode==='path'){let cur=-1;for(let k=0;k<n;k++){const i=R.order[k];if(R.c[i]!==cur){if(cur>=0)ctx.fill();cur=R.c[i];ctx.fillStyle=COLS[cur];ctx.beginPath()}ctx.rect((R.x[i]+off)%1600,R.y[i],1.5,R.h[i])}ctx.fill()}
 else if(mode==='text'){ctx.fillStyle='#222';ctx.font='12px system-ui,sans-serif';for(let i=0;i<n;i++)ctx.fillText('svc-'+(i%97)+' 12:03:'+(i%60),(R.x[i]+off)%1500,R.y[i]+12)}
};
self.gl2=function(gl,R,N){const vs=`#version 300 es
in vec2 q;in vec4 r;in float k;uniform float off;out vec3 col;
const vec3 C[3]=vec3[3](vec3(.2,.4,.8),vec3(.8,.47,.2),vec3(.8,.2,.33));
void main(){float px=mod(r.x+off,1600.);vec2 p=vec2(px,r.y)+q*vec2(r.z,r.w);gl_Position=vec4(p.x/800.-1.,1.-p.y/450.,0,1);col=C[int(k)];}`;
 const fs=`#version 300 es
precision mediump float;in vec3 col;out vec4 o;void main(){o=vec4(col,1);}`;
 const sh=(t,s)=>{const x=gl.createShader(t);gl.shaderSource(x,s);gl.compileShader(x);if(!gl.getShaderParameter(x,gl.COMPILE_STATUS))throw new Error(gl.getShaderInfoLog(x));return x};
 const p=gl.createProgram();gl.attachShader(p,sh(gl.VERTEX_SHADER,vs));gl.attachShader(p,sh(gl.FRAGMENT_SHADER,fs));gl.linkProgram(p);gl.useProgram(p);
 const vao=gl.createVertexArray();gl.bindVertexArray(vao);
 const qb=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,qb);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([0,0,1,0,0,1,1,1]),gl.STATIC_DRAW);
 const ql=gl.getAttribLocation(p,'q');gl.enableVertexAttribArray(ql);gl.vertexAttribPointer(ql,2,gl.FLOAT,false,0,0);
 const a=new Float32Array(N*5);for(let i=0;i<N;i++){a[i*5]=R.x[i];a[i*5+1]=R.y[i];a[i*5+2]=1.5;a[i*5+3]=R.h[i];a[i*5+4]=R.c[i]}
 const ib=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,ib);gl.bufferData(gl.ARRAY_BUFFER,a,gl.STATIC_DRAW);
 const rl=gl.getAttribLocation(p,'r'),kl=gl.getAttribLocation(p,'k');
 gl.enableVertexAttribArray(rl);gl.vertexAttribPointer(rl,4,gl.FLOAT,false,20,0);gl.vertexAttribDivisor(rl,1);
 gl.enableVertexAttribArray(kl);gl.vertexAttribPointer(kl,1,gl.FLOAT,false,20,16);gl.vertexAttribDivisor(kl,1);
 return gl.getUniformLocation(p,'off')};
self.loop=function(raf,step,frames,maxMs){return new Promise(res=>{const t=[],js=[];let last=0,f=0;const t0=performance.now();
 const tick=(now)=>{const a=performance.now();step(f);js.push(performance.now()-a);if(last)t.push(now-last);last=now;f++;
  if(f>=frames||performance.now()-t0>maxMs)return res(summ(t,js));raf(tick)};raf(tick)})};
self.summ=function(t,js){const q=(v,p)=>{const s=[...v].sort((a,b)=>a-b);return s.length?+s[Math.min(s.length-1,Math.floor(p*s.length))].toFixed(2):null};
 return {frames:t.length,rafP50:q(t,.5),rafP95:q(t,.95),jsP50:q(js,.5),jsP95:q(js,.95)}};
