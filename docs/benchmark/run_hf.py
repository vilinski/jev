"""Run a seeded sample of the public jev-bench (PER test items from each of its 22 datasets; default 50, so 1,100 items) through one provider.

    python3 run_hf.py <typesafe|celeris|cloudflare|openai|openai-chat> results/hf_<name>.json [per-dataset-count]

The datasets are downloaded once from https://huggingface.co/datasets/Praveenrajus/jev-bench (licence: mixed, see its
manifest) into ./jev-bench-data. One call per item, sequential; an extra warm-up call is discarded.
"""
import json, sys, random, glob, os
from fetch_jevbench import fetch
from jevcall import call
prov, out = sys.argv[1], sys.argv[2]
PER = int(sys.argv[3]) if len(sys.argv) > 3 else int(os.environ.get("JEVBENCH_PER", "50"))   # test items per dataset (22 datasets)
HF = os.environ.get("JEVBENCH_DIR", os.path.join(os.path.dirname(os.path.abspath(__file__)), "jev-bench-data"))
HF = fetch(HF)
rng = random.Random(7)
items = []
for f in sorted(glob.glob(f"{HF}/*.jsonl")):
    lines = list(open(f))   # not splitlines(): it also splits on Unicode separators inside JSON strings
    items.extend(json.loads(l) for l in rng.sample(lines, PER))   # parse only the sampled rows
call(prov, {"state": "warm up", "questions": {"q": {"type": "noul", "instructions": "Is this a test?"}}}, retries=1, timeout=180)  # warm-up, discarded
rows = []
for i, it in enumerate(items):
    try: state = json.loads(it["state"])
    except Exception: state = it["state"]
    q = json.loads(it["question"])
    o = call(prov, {"state": state, "questions": {"q": q}}, retries=1, timeout=180)
    o.update(id=it["id"], source=it["source"], kind=q["type"], label=it["label"], options=list(q["criteria"]) if isinstance(q.get("criteria"), dict) else None)
    rows.append(o)
    print(prov, i, it["source"], round(o["dt"], 2), o["ok"], flush=True)
json.dump(dict(prov=prov, rows=rows), open(out, "w"))
