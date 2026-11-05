importScripts('shared.js');
onmessage=async(e)=>{const m=e.data;
 if(m.kind==='caps'){const out={};
  try{const c=new OffscreenCanvas(8,8);out.w2d=!!c.getContext('2d')}catch(x){out.w2d='throw '+x.name}
  try{const c=new OffscreenCanvas(8,8);out.wgl2=!!c.getContext('webgl2')}catch(x){out.wgl2='throw '+x.name}
  out.wRaf=typeof requestAnimationFrame==='function';out.wgpu=!!self.navigator.gpu;postMessage(out);return}
 if(m.kind==='init'){self.cv=m.canvas;self.ctx=cv.getContext('2d',{alpha:false});ctx.setTransform(m.dpr,0,0,m.dpr,0,0);postMessage({ok:true});return}
 if(m.kind==='run'){const R=mkRects(m.n);
  const raf=typeof requestAnimationFrame==='function'?requestAnimationFrame.bind(self):(f)=>setTimeout(()=>f(performance.now()),16);
  const r=await loop(raf,(f)=>{ctx.fillStyle='#fff';ctx.fillRect(0,0,1600,900);draw2d(ctx,R,m.n,'grouped',f*3)},m.frames,m.maxMs);
  r.pixel=Array.from(ctx.getImageData(0,0,1600,900).data.slice(0,4));postMessage(r)}
};
