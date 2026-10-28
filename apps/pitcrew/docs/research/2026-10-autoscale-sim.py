# Synthetic CI queue: bursts of jobs (a push fans out to several jobs), lognormal durations.
# Compares static capacity with an autoscaler with and without hysteresis. Time step 5 s, 8 simulated hours.
import random, math, statistics, sys
def workload(seed):
    r=random.Random(seed); jobs=[]; t=0
    while t<8*3600:
        t+=r.expovariate(1/600)          # a push every ~10 min on average
        n=r.choice([1,2,3,4,6,8])        # jobs per push
        for _ in range(n): jobs.append((t, max(60, r.lognormvariate(math.log(360),0.6))))
    return jobs
def run(jobs, mode, mn=1, mx=4, up_delay=20, down_delay=600, start_gap=15, boot=20, poll=15):
    dt=5; t=0; q=[]; ji=0; slots=[{'on':i<mn,'busy':0,'idle_since':0,'boot':0} for i in range(mx)]
    if mode=='static': 
        for s in slots[:mn]: s['on']=True
    waits=[]; on_secs=0; actions=0; want_since=None; last_start=-1e9; last_poll=-1e9; qlen=0
    while t<9*3600:
        while ji<len(jobs) and jobs[ji][0]<=t: q.append(jobs[ji]); ji+=1
        if mode!='static' and t-last_poll>=poll: qlen=len(q); last_poll=t
        for s in slots:
            if s['busy']>0:
                s['busy']-=dt
                if s['busy']<=0: s['idle_since']=t
        for s in slots:
            if s['on'] and s['boot']<=t and s['busy']<=0 and q:
                a,d=q.pop(0); waits.append(t-a); s['busy']=d
        if mode in ('auto','nohyst'):
            on=[s for s in slots if s['on']]; busy=sum(1 for s in on if s['busy']>0)
            target=min(mx,max(mn,busy+qlen))
            if target>len(on):
                want_since = want_since if want_since is not None else t
                ud = up_delay if mode=='auto' else 0; gap = start_gap if mode=='auto' else 0
                if t-want_since>=ud and t-last_start>=gap:
                    s=next(s for s in slots if not s['on']); s['on']=True; s['boot']=t+boot; s['idle_since']=t; last_start=t; actions+=1
            else: want_since=None
            dd = down_delay if mode=='auto' else 0
            if target<len(on):
                idle=[s for s in on if s['busy']<=0 and t-s['idle_since']>=dd and s['boot']<=t]
                if idle and len(on)>mn: idle[-1]['on']=False; actions+=1
        on_secs+=sum(dt for s in slots if s['on']); t+=dt
    return statistics.mean(waits), sorted(waits)[int(.95*len(waits))], on_secs/3600, actions
res={}
for mode,kw in [('static',dict(mn=1)),('static',dict(mn=4)),('auto',{}),('nohyst',{})]:
    rows=[run(workload(s),mode,**kw) for s in range(30)]
    name=mode+(str(kw.get('mn','')) if mode=='static' else '')
    print(f"{name:8s} mean wait {statistics.mean(r[0] for r in rows)/60:6.1f} min  p95 {statistics.mean(r[1] for r in rows)/60:6.1f} min  slot-hours {statistics.mean(r[2] for r in rows):5.1f}  start/stop actions {statistics.mean(r[3] for r in rows):6.1f}")
