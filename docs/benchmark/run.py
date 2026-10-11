import sys
import json, subprocess, time, statistics, os
from dataset import *
prov, reps, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
def build():
    items=[]
    for i,(t,q,g) in enumerate(N): items.append(dict(id=f"n{i}",kind="noul",state={"text":t},q={"type":"noul","instructions":q},gold=g))
    for i,(t,g) in enumerate(C): items.append(dict(id=f"c{i}",kind="choice",state={"text":t},q={"type":"choice","instructions":"Which team should handle this ticket?","criteria":{**ROUTE,"unclear":"Not determinable from the text, or spans several areas"}},gold=g))
    for i,(t,g) in enumerate(S): items.append(dict(id=f"s{i}",kind="score",state={"text":t},q={"type":"score","instructions":"How severe is this for a production customer?","criteria":SEV},gold=g))
    return items
def call(body):
    t=time.perf_counter()
    p=subprocess.run(["jev","run","-","-p",prov,"--retries","0"],input=json.dumps(body),capture_output=True,text=True,timeout=120)
    dt=time.perf_counter()-t
    usage=None
    for l in p.stderr.splitlines():
        if l.startswith("usage:"): usage=l[6:].strip()
    try: res=json.loads(p.stdout)
    except Exception: res=None
    return dict(ok=p.returncode==0 and res is not None,dt=dt,res=res,usage=usage,err=None if p.returncode==0 else p.stderr[-300:])
items=build(); rows=[]
for r in range(reps):
    for it in items:
        o=call({"state":it["state"],"questions":{"q":it["q"]}})
        o.update(id=it["id"],kind=it["kind"],gold=it["gold"],rep=r); rows.append(o)
        print(prov,r,it["id"],round(o["dt"],2),o["ok"],flush=True)
# batching test: 10 questions about one state, batched vs separate
st={"report":"Data stops arriving after ~6h; restarting the pod fixes it. Customers on the EU cluster notice gaps in dashboards."}
qs={f"q{i}":{"type":"noul","instructions":q} for i,q in enumerate(["Is this a resource leak?","Is this a billing issue?","Is a restart a workaround?","Does it affect production customers?","Is data lost permanently?","Is it a UI bug?","Does it need a human decision?","Is the root cause known?","Is it intermittent?","Is it region specific?"])}
batch=[call({"state":st,"questions":qs}) for _ in range(3)]
sep=[sum(call({"state":st,"questions":{k:v}})["dt"] for k,v in qs.items()) for _ in range(2)]
json.dump(dict(prov=prov,rows=rows,batch=[b["dt"] for b in batch],batch_ok=[b["ok"] for b in batch],sep=sep),open(out,"w"))
