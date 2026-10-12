"""Scoring of the benchmark results: the 60-item own set and the public jev-bench sample."""
import collections, json, random, re, statistics as st
from collections import Counter

from dataset import C, N, S
from models import MODELS, RESULTS

def pct(v, q):
    v = sorted(v); k = (len(v) - 1) * q; f = int(k); c = min(f + 1, len(v) - 1)
    return v[f] + (v[c] - v[f]) * (k - f)

def is_warmup(row):
    """The first call of a run pays a cold start and is left out of latency. Old results have no flag."""
    return row.get("warmup") or (row["id"] == "n0" and row["rep"] == 0)

# ---------------------------------------------------------------- own set (60 labelled items, 3 runs)

def analyse_own(r):
    out = {}
    rows = r["rows"]; ok = [x for x in rows if x["ok"]]
    lat = [x["dt"] for x in ok if not is_warmup(x)]
    out["n"] = len(rows); out["err"] = len(rows) - len(ok)
    out["lat"] = dict(p50=pct(lat, .5), p95=pct(lat, .95), mean=st.mean(lat), mx=max(lat), min=min(lat), all=lat)
    tin = tout = 0
    for x in ok:
        m = re.match(r"(\d+) in / (\d+) out", x["usage"] or "")
        if m: tin += int(m[1]); tout += int(m[2])
    out["tok"] = (tin / len(ok), tout / len(ok))
    brier = {"noul": [], "choice": []}; conf_ok = []; conf_bad = []; sel = []; amb = []; mae = []
    kinds = {"noul": [], "choice": [], "score_exact": [], "score_pm1": []}
    byid = {}
    for x in ok:
        a = x["res"]["q"]; byid.setdefault(x["id"], []).append((x, a))
        k = x["kind"]; g = x["gold"]
        if k == "noul":
            p = a["noul"]
            if g is None: amb.append(abs(p - .5)); continue
            hit = (p > .5) == g; kinds["noul"].append(hit)
            brier["noul"].append((p - (1 if g else 0)) ** 2)
            c = max(p, 1 - p)
        elif k == "choice":
            pr = a["probabilities"]; hit = a["choice"] == g; kinds["choice"].append(hit)
            brier["choice"].append(sum((pr.get(o, 0) - (1 if o == g else 0)) ** 2 for o in set(pr) | {g}))
            c = a["confidence"]
        else:
            pr = a["probabilities"]; top = max(pr, key=pr.get); hit = int(top) == g
            kinds["score_exact"].append(hit); kinds["score_pm1"].append(abs(a["score"] - g) <= 0.75)
            mae.append(abs(a["score"] - g))
            c = a["confidence"]
        (conf_ok if hit else conf_bad).append(c); sel.append((c, hit))
    out["acc"] = {k: (sum(v) / len(v), len(v)) for k, v in kinds.items()}
    out["acc_all"] = sum(h for _, h in sel) / len(sel)
    out["brier"] = {k: st.mean(v) for k, v in brier.items()}
    out["mae"] = st.mean(mae)
    out["conf_ok"] = st.mean(conf_ok); out["conf_bad"] = st.mean(conf_bad)
    hi = [h for c, h in sel if c >= .8]; lo = [h for c, h in sel if c < .8]
    out["hi"] = (sum(hi) / len(hi) if hi else 0, len(hi)); out["lo"] = (sum(lo) / len(lo) if lo else 0, len(lo))
    out["amb_dist"] = st.mean(amb); out["amb_abstain"] = sum(d < .25 for d in amb) / len(amb)   # p within .25..0.75
    sd = []; flip = []
    for v in byid.values():
        if len(v) < 2: continue
        vals = [a["noul"] if x["kind"] == "noul" else a["score"] if x["kind"] == "score" else a["confidence"] for x, a in v]
        sd.append(st.pstdev(vals))
        flip.append(len(set(decision(x["kind"], a) for x, a in v)) > 1)
    out["sd"] = st.mean(sd); out["flip"] = sum(flip) / len(flip)
    out["batch"] = st.mean(r["batch"]); out["sep"] = st.mean(r["sep"])
    out["byid"] = byid   # per-item answers for the list of wrong items
    return out

def decision(kind, a):
    """What the model decided: yes/no, the option, or the top level."""
    if kind == "noul": return a["noul"] > .5
    if kind == "choice": return a["choice"]
    return max(a["probabilities"], key=a["probabilities"].get)

def own_item(item_id):
    """(text, ..., gold) tuple of an own-set item id such as `n3`, `c12`, `s5`."""
    return {"n": N, "c": C, "s": S}[item_id[0]][int(item_id[1:])]

def describe(a, kind):
    if kind == "noul": return ("yes" if a["noul"] > .5 else "no") + f" ({a['noul']:.2f})"
    if kind == "choice": return f"{a['choice']} ({a['confidence']:.2f})"
    return f"L{max(a['probabilities'], key=a['probabilities'].get)} · {a['score']:.2f}"

def majority(answers, kind):
    dec = [decision(kind, a) if kind != "score" else int(decision(kind, a)) for _, a in answers]
    return Counter(dec).most_common(1)[0][0]

def wrong_items(R, keys):
    """[(item text, gold, [(model key, description, wrong?)])] for items some model got wrong (majority of its runs)."""
    out = []
    for i in R[keys[0]]["byid"]:
        kind = {"n": "noul", "c": "choice", "s": "score"}[i[0]]; it = own_item(i); gold = it[-1]
        if gold is None: continue
        wrong = [majority(R[k]["byid"][i], kind) != gold for k in keys]
        if any(wrong):
            out.append((it[0], gold, [(k, describe(R[k]["byid"][i][0][1], kind), w) for k, w in zip(keys, wrong)]))
    return out

# ---------------------------------------------------------------- public jev-bench sample

def score_row(r):
    """(accuracy credit 0..1, Brier sum over options, option count); failed or refused answers score as uniform."""
    k = r["kind"]; a = (r["res"] or {}).get("q") if r["ok"] else None
    if a is not None and a.get("type") == "refusal": a = None
    if k == "noul":
        gold = 1.0 if str(r["label"]) == "1" else 0.0
        if a is None: return .5, .5, 2   # uniform: sum (0.5-g)^2*2
        p = a["noul"]; acc = 1.0 if (p > .5) == (gold == 1.0) else (0.5 if p == .5 else 0.0)
        return acc, (p - gold) ** 2 + ((1 - p) - (1 - gold)) ** 2, 2
    if a is None and k == "score": return None, None, None
    if k == "choice":
        opts = r["options"]; gold = str(r["label"])
    else:
        gold = str(int(r["label"])); opts = None
    if a is None:
        n = len(opts); return 1 / n, (1 - 1 / n) ** 2 + (n - 1) * (1 / n) ** 2, n
    pr = a["probabilities"]
    if opts is None: opts = [str(i) for i in range(len(pr))]
    vec = [pr.get(o, 0.0) for o in opts]; s = sum(vec)
    if s > 0 and abs(s - 1) <= .05: vec = [v / s for v in vec]
    b = sum((v - (1.0 if o == gold else 0.0)) ** 2 for o, v in zip(opts, vec))
    m = max(vec); top = [o for o, v in zip(opts, vec) if v == m]
    return (1 / len(top) if gold in top else 0.0), b, len(opts)

def analyse_public(d):
    rows = d["rows"]; acc = []; br = []; per = collections.defaultdict(list); fail = 0; credits = []   # credits: one entry per row, None where the row could not be scored
    for r in rows:
        refused = r["ok"] and (r["res"]["q"].get("type") == "refusal")
        if not r["ok"] or refused:
            fail += 1
            if r["kind"] == "score":   # the level count of a failed score row is unknown
                credits.append(None); continue
        s = score_row(r)
        credits.append(s[0])
        if s[0] is None: continue
        acc.append(s[0]); br.append(s[1]); per[r["source"]].append(s[0])
    lat = [r["dt"] for r in rows if r["ok"]]
    toks = [int(r["usage"].split()[0]) for r in rows if r["ok"] and r["usage"]]
    return dict(accs=acc, credits=credits, n=len(rows), fail=fail, acc=st.mean(acc), brier=st.mean(br), per={k: st.mean(v) for k, v in per.items()},
                p50=pct(lat, .5), p95=pct(lat, .95), mx=max(lat), tok=st.mean(toks) if toks else 0)

def bootstrap_cis(values_by_key, runs=2000, seed=1):
    """{key: (low, high)}: 95% interval of each mean from resampling; one seeded generator, drawn in key order."""
    rng = random.Random(seed)
    out = {}
    for key, values in values_by_key.items():
        bs = sorted(sum(rng.choices(values, k=len(values))) / len(values) for _ in range(runs))
        out[key] = (bs[int(runs * .025)], bs[int(runs * .975)])
    return out

def paired_cis(credits_by_key, pairs, runs=2000, seed=1):
    """{(a, b): (mean difference a-b, low, high)}: 95% interval of the accuracy difference on the items both models were scored on.

    Both models answered the same items, so resampling items jointly gives a much tighter interval than comparing two separate intervals.
    """
    rng = random.Random(seed)
    out = {}
    for a, b in pairs:
        diffs = [x - y for x, y in zip(credits_by_key[a], credits_by_key[b]) if x is not None and y is not None]
        ms = sorted(sum(rng.choices(diffs, k=len(diffs))) / len(diffs) for _ in range(runs))
        out[(a, b)] = (sum(diffs) / len(diffs), ms[int(runs * .025)], ms[int(runs * .975)])
    return out

# ---------------------------------------------------------------- loading

def load_all(models=MODELS):
    """({key: own-set analysis}, {key: public-sample analysis})."""
    own = {m.key: analyse_own(json.load(open(f"{RESULTS}/{m.stem}.json"))) for m in models}
    pub = {m.key: analyse_public(json.load(open(f"{RESULTS}/hf_{m.stem}.json"))) for m in models}
    return own, pub
