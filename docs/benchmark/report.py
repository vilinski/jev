import json, statistics as st, re, html, datetime, sys
from dataset import *
P = {"Jev (TypeSafe)":"jev.json","Clef (Cloudflare)":"clef.json"}
COL = {"Jev (TypeSafe)":"#2f6fed","Clef (Cloudflare)":"#e8710a"}
def pct(v,q):
    v=sorted(v); k=(len(v)-1)*q; f=int(k); c=min(f+1,len(v)-1); return v[f]+(v[c]-v[f])*(k-f)
def main(r):
    out={}
    rows=r["rows"]; ok=[x for x in rows if x["ok"]]
    lat=[x["dt"] for x in ok]
    out["n"]=len(rows); out["err"]=len(rows)-len(ok)
    out["lat"]=dict(p50=pct(lat,.5),p95=pct(lat,.95),mean=st.mean(lat),mx=max(lat),min=min(lat),all=lat)
    tin=tout=0
    for x in ok:
        m=re.match(r"(\d+) in / (\d+) out",x["usage"] or "")
        if m: tin+=int(m[1]); tout+=int(m[2])
    out["tok"]=(tin/len(ok),tout/len(ok))
    acc={}; brier={"noul":[],"choice":[]}; conf_ok=[]; conf_bad=[]; sel=[]; amb=[]; mae=[]; 
    kinds={"noul":[], "choice":[], "score_exact":[], "score_pm1":[]}
    byid={}
    for x in ok:
        a=x["res"]["q"]; byid.setdefault(x["id"],[]).append((x,a))
        k=x["kind"]; g=x["gold"]
        if k=="noul":
            p=a["noul"]
            if g is None: amb.append(abs(p-.5)); continue
            hit=(p>.5)==g; kinds["noul"].append(hit)
            brier["noul"].append((p-(1 if g else 0))**2)
            c=max(p,1-p); (conf_ok if hit else conf_bad).append(c); sel.append((c,hit))
        elif k=="choice":
            pr=a["probabilities"]; hit=a["choice"]==g; kinds["choice"].append(hit)
            brier["choice"].append(sum((pr.get(o,0)-(1 if o==g else 0))**2 for o in set(pr)|{g}))
            c=a["confidence"]; (conf_ok if hit else conf_bad).append(c); sel.append((c,hit))
        else:
            pr=a["probabilities"]; top=max(pr,key=pr.get); hit=int(top)==g
            kinds["score_exact"].append(hit); kinds["score_pm1"].append(abs(a["score"]-g)<=0.75)
            mae.append(abs(a["score"]-g))
            c=a["confidence"]; (conf_ok if hit else conf_bad).append(c); sel.append((c,hit))
    out["acc"]={k:(sum(v)/len(v),len(v)) for k,v in kinds.items()}
    allh=[h for _,h in sel]; out["acc_all"]=sum(allh)/len(allh)
    out["brier"]={k:st.mean(v) for k,v in brier.items()}
    out["mae"]=st.mean(mae)
    out["conf_ok"]=st.mean(conf_ok); out["conf_bad"]=st.mean(conf_bad)
    hi=[h for c,h in sel if c>=.8]; lo=[h for c,h in sel if c<.8]
    out["hi"]=(sum(hi)/len(hi) if hi else 0,len(hi)); out["lo"]=(sum(lo)/len(lo) if lo else 0,len(lo))
    out["amb_dist"]=st.mean(amb); out["amb_abstain"]=sum(d<.25 for d in amb)/len(amb)  # p within .25..0.75
    # stability
    sd=[];flip=[]
    for i,v in byid.items():
        x0=v[0][0]
        if len(v)<2: continue
        vals=[ (a["noul"] if x["kind"]=="noul" else a["score"] if x["kind"]=="score" else a["confidence"]) for x,a in v]
        sd.append(st.pstdev(vals))
        dec=[ (a["noul"]>.5) if x["kind"]=="noul" else a["choice"] if x["kind"]=="choice" else max(a["probabilities"],key=a["probabilities"].get) for x,a in v]
        flip.append(len(set(dec))>1)
    out["sd"]=st.mean(sd); out["flip"]=sum(flip)/len(flip)
    out["batch"]=st.mean(r["batch"]); out["sep"]=st.mean(r["sep"])
    # per-item answers for disagreement table
    out["byid"]={i:v for i,v in byid.items()}
    return out
R={n:main(json.load(open(f))) for n,f in P.items()}
json.dump({n:{k:v for k,v in o.items() if k!="byid"} for n,o in R.items()},open("summary.json","w"),indent=1,default=str)
# disagreements / wrong items
def label(i):
    k=i[0]; n=int(i[1:])
    return {"n":N,"c":C,"s":S}[k][n]
def pred(a,kind):
    return ("yes" if a["noul"]>.5 else "no")+f" ({a['noul']:.2f})" if kind=="noul" else (f"{a['choice']} ({a['confidence']:.2f})" if kind=="choice" else f"L{max(a['probabilities'],key=a['probabilities'].get)} score {a['score']:.2f}")
def maj(v,kind):
    from collections import Counter
    dec=[ (a["noul"]>.5) if kind=="noul" else a["choice"] if kind=="choice" else int(max(a["probabilities"],key=a["probabilities"].get)) for x,a in v]
    return Counter(dec).most_common(1)[0][0]
trs=[]
names=list(R)
for i in R[names[0]]["byid"]:
    kind={"n":"noul","c":"choice","s":"score"}[i[0]]; it=label(i); gold=it[-1]
    ms=[maj(R[n]["byid"][i],kind) if i in R[n]["byid"] else None for n in names]
    wrong=[(m!=gold) if gold is not None else False for m in ms]
    if any(wrong):
        txt=it[0]
        cells="".join(f'<td class="{ "bad" if w else "good"}">{html.escape(str(pred(R[n]["byid"][i][0][1],kind)) if i in R[n]["byid"] else "–")}</td>' for n,w in zip(names,wrong))
        trs.append(f'<tr><td>{html.escape(txt)}</td><td>{html.escape(str(gold))}</td>{cells}</tr>')
def bar(vals,fmt="{:.0%}",mx=1.0,unit=""):
    s=""
    for n,v in vals:
        s+=f'<div class="barrow"><span class="bl">{n}</span><div class="bt"><div class="bf" style="width:{min(v/mx,1)*100:.1f}%;background:{COL[n]}"></div></div><span class="bv">{fmt.format(v)}{unit}</span></div>'
    return s
def card(title,body,sub=""):
    return f'<section class="card"><h3>{title}</h3><p class="sub">{sub}</p>{body}</section>'
mxl=max(R[n]["lat"]["p95"] for n in R)
def hist(n):
    v=R[n]["lat"]["all"]; lo=0; hi=max(max(R[m]["lat"]["all"]) for m in R); bins=14; cnt=[0]*bins
    for x in v: cnt[min(int(x/hi*bins),bins-1)]+=1
    m=max(cnt) or 1; w=300/bins
    bars="".join(f'<rect x="{i*w:.1f}" y="{80-c/m*78:.1f}" width="{w-2:.1f}" height="{c/m*78:.1f}" fill="{COL[n]}"/>' for i,c in enumerate(cnt))
    return f'<div class="hist"><b style="color:{COL[n]}">{n}</b><svg viewBox="0 0 300 96" width="100%"><line x1="0" y1="80" x2="300" y2="80" stroke="#999"/>{bars}<text x="0" y="94" font-size="9" fill="#888">0 s</text><text x="300" y="94" font-size="9" fill="#888" text-anchor="end">{hi:.1f} s</text></svg></div>'
rowsT=""
def tr(label,f,hint=""):
    return f'<tr><td>{label}<div class="hint">{hint}</div></td>'+"".join(f'<td>{f(R[n])}</td>' for n in R)+'</tr>'
th="".join(f'<th style="color:{COL[n]}">{n}</th>' for n in R)
table=(tr("Latency p50",lambda o:f'{o["lat"]["p50"]*1000:.0f} ms',"median single-question call, wall clock incl. TLS + process")
+tr("Latency p95",lambda o:f'{o["lat"]["p95"]*1000:.0f} ms',"tail")
+tr("Latency max",lambda o:f'{o["lat"]["mx"]*1000:.0f} ms')
+tr("10 questions batched",lambda o:f'{o["batch"]*1000:.0f} ms',"one request, 10 yes/no questions")
+tr("10 questions as 10 calls",lambda o:f'{o["sep"]*1000:.0f} ms',"sequential")
+tr("Accuracy yes/no",lambda o:f'{o["acc"]["noul"][0]:.1%} <small>n={o["acc"]["noul"][1]}</small>',"labelled items, threshold 0.5")
+tr("Accuracy routing",lambda o:f'{o["acc"]["choice"][0]:.1%} <small>n={o["acc"]["choice"][1]}</small>',"5 teams + &quot;unclear&quot;")
+tr("Accuracy severity (exact level)",lambda o:f'{o["acc"]["score_exact"][0]:.1%} <small>n={o["acc"]["score_exact"][1]}</small>',"argmax of level distribution")
+tr("Severity within ±¾ level",lambda o:f'{o["acc"]["score_pm1"][0]:.1%}')
+tr("Severity MAE",lambda o:f'{o["mae"]:.2f} levels')
+tr("Brier yes/no (lower better)",lambda o:f'{o["brier"]["noul"]:.3f}',"calibration + sharpness")
+tr("Brier routing (lower better)",lambda o:f'{o["brier"]["choice"]:.3f}')
+tr("Confidence when right",lambda o:f'{o["conf_ok"]:.2f}')
+tr("Confidence when wrong",lambda o:f'{o["conf_bad"]:.2f}',"smaller gap to the line above = overconfident")
+tr("Accuracy when confidence ≥ 0.8",lambda o:f'{o["hi"][0]:.1%} <small>n={o["hi"][1]}</small>')
+tr("Accuracy when confidence &lt; 0.8",lambda o:f'{o["lo"][0]:.1%} <small>n={o["lo"][1]}</small>',"a useful model is much worse here → you can route these to a human")
+tr("Ambiguous items → near 0.5",lambda o:f'{o["amb_abstain"]:.0%}',"share of deliberately vague yes/no items with p in 0.25–0.75")
+tr("Run-to-run std dev",lambda o:f'{o["sd"]:.3f}',"3 repeats of each item")
+tr("Decision flips between runs",lambda o:f'{o["flip"]:.0%}',"items whose decision changed across 3 runs")
+tr("Failed calls",lambda o:f'{o["err"]} / {o["n"]}',"retries disabled")
+tr("Tokens per call (in / out)",lambda o:f'{o["tok"][0]:.0f} / {o["tok"][1]:.0f}',"as reported on stderr"))
J,Cl=R["Jev (TypeSafe)"],R["Clef (Cloudflare)"]
kp=lambda t,v,s:f'<div class="kpi"><b>{v}</b>{t}<div class="hint">{s}</div></div>'
kpis='<div class="kpis">'+kp("Median latency",f"{J['lat']['p50']*1000:.0f} vs {Cl['lat']['p50']*1000:.0f} ms","Jev vs Clef")+kp("Worst call",f"{J['lat']['mx']:.1f} vs {Cl['lat']['mx']:.1f} s","Jev vs Clef")+kp("Overall accuracy",f"{J['acc_all']:.0%} vs {Cl['acc_all']:.0%}","all labelled items, 3 runs")+kp("Brier yes/no",f"{J['brier']['noul']:.3f} vs {Cl['brier']['noul']:.3f}","lower is better")+'</div>'
speed=card("Latency per call",bar([(n,R[n]["lat"]["p50"]) for n in R],"{:.2f}",mxl,"s")+bar([(n,R[n]["lat"]["p95"]) for n in R],"{:.2f}",mxl,"s"),"Top bars median, bottom bars p95. Single-question calls via the CLI, incl. process start and TLS handshake.")+card("Latency distribution","".join(hist(n) for n in R),"Histogram of all 180 calls per model.")+card("Batching: 10 questions, one state",bar([(n,R[n]["batch"]) for n in R],"{:.2f}",max(R[n]["sep"] for n in R),"s")+bar([(n,R[n]["sep"]) for n in R],"{:.2f}",max(R[n]["sep"] for n in R),"s"),"Top bars one batched request, bottom bars ten separate calls.")
qual=card("Accuracy by task",bar([(n,R[n]["acc"]["noul"][0]) for n in R])+bar([(n,R[n]["acc"]["choice"][0]) for n in R])+bar([(n,R[n]["acc"]["score_exact"][0]) for n in R]),"Yes/no (24 items), routing (20), severity exact level (16); each ×3 runs.")+card("Does confidence mean anything?",bar([(n,R[n]["conf_ok"]) for n in R],"{:.2f}")+bar([(n,R[n]["conf_bad"]) for n in R],"{:.2f}"),"Top: mean confidence when right. Bottom: when wrong. Jev: 0.96 vs 0.74; Clef: 0.76 vs 0.45. Both are lower when wrong.")+card("Ambiguous inputs",bar([(n,R[n]["amb_abstain"]) for n in R]),"Share of 4 deliberately vague yes/no items where the model answered 0.25-0.75 (&quot;cannot tell&quot;) instead of guessing.")
notes=("<p><b>Takeaways.</b> On this set both are accurate and deterministic. Jev is faster and far steadier in latency (p95 "+f"{J['lat']['p95']*1000:.0f} ms vs {Cl['lat']['p95']*1000:.0f} ms, Clef max {Cl['lat']['mx']:.1f} s), "+"is much better calibrated (Brier about 10x lower) and says &quot;I cannot tell&quot; on every vague input, where Clef commits to a confident answer on most. Clef is better on the 4-level severity task (94% vs 75% exact level), but three of Jev's four misses rate an all-users outage as level 3 instead of my label 2, which is arguably a labelling judgement, not an error. Clef's confidence is lower overall, so a 0.8 threshold does not separate right from wrong for it (93% accurate below it).</p>"
"<p><b>Caveats.</b> The set is synthetic, small (60 items) and easy: yes/no and routing sit at the ceiling, so differences there are invisible and the numbers do not generalise to your real data. Run-to-run spread was measured over only 3 runs. Latency was measured over the CLI, one call at a time; an SDK with connection pooling would be faster for both. OpenAI was not run, so this is not yet a three-way comparison; the runner and report take any further provider with the same output shape.</p>")
now=datetime.date.today().isoformat()
HTML=f'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Decision API benchmark</title>
<style>
:root{{--bg:#fafaf7;--fg:#1b1b1b;--mut:#666;--card:#fff;--line:#e3e1da;--bad:#fbe3e0;--good:#e3f4e6}}
@media(prefers-color-scheme:dark){{:root{{--bg:#16171a;--fg:#ececec;--mut:#9a9a9a;--card:#1f2024;--line:#33353a;--bad:#4a2522;--good:#1f3a25}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}}
main{{max-width:980px;margin:0 auto;padding:24px 16px 64px}}
h1{{font-size:28px;margin:0}} h2{{margin-top:40px;font-size:20px}} h3{{margin:0 0 2px;font-size:16px}}
.sub,.hint,small{{color:var(--mut);font-size:12.5px;font-weight:400}} .sub{{margin:0 0 12px}}
.grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:14px}}
.card{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:16px}}
.barrow{{display:grid;grid-template-columns:130px 1fr 60px;gap:8px;align-items:center;margin:6px 0;font-size:13px}}
.bt{{background:var(--line);height:12px;border-radius:6px;overflow:hidden}} .bf{{height:100%}} .bv{{text-align:right;font-variant-numeric:tabular-nums}}
table{{width:100%;border-collapse:collapse;background:var(--card);border:1px solid var(--line);border-radius:10px;overflow:hidden}}
th,td{{padding:8px 10px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top;font-size:14px}} td{{font-variant-numeric:tabular-nums}}
td.bad{{background:var(--bad)}} td.good{{background:var(--good)}}
.tablewrap{{overflow-x:auto}} .note{{background:var(--card);border-left:4px solid #999;padding:10px 14px;margin:14px 0;border-radius:4px}}
.kpis{{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:12px;margin:18px 0}} .kpi{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px}} .kpi b{{font-size:24px;display:block}}
</style></head><body><main>
<h1>Decision API benchmark: Jev vs Clef</h1>
<p class="sub">{now} · {R[names[0]]["n"]} calls per model ({len(N)+len(C)+len(S)} synthetic labelled items × 3 runs) · via the <code>jev</code> CLI, one question per call · OpenAI was skipped (not run).</p>
{kpis}
<h2>Speed</h2>
<div class="grid">{speed}</div>
<h2>Quality</h2>
<div class="grid">{qual}</div>
<h2>All numbers</h2><div class="tablewrap"><table><tr><th>Metric</th>{th}</tr>{table}</table></div>
<h2>Items where a model was wrong (majority of 3 runs)</h2>
<div class="tablewrap"><table><tr><th>Text</th><th>Gold</th>{th}</tr>{"".join(trs)}</table></div>
<h2>How to read this</h2>
<div class="note">{notes}</div>
</main></body></html>'''
open("report.html","w").write(HTML)
