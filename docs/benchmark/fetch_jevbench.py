"""Download the test splits of the public jev-bench (https://huggingface.co/datasets/Praveenrajus/jev-bench) once.

    python3 fetch_jevbench.py [directory]     # default ./jev-bench-data

Files are fetched in parallel and moved into place only when complete, so concurrent runners never see a partial file.
Licence of the data: mixed, see the dataset's manifest.
"""
import os, sys, urllib.request
from concurrent.futures import ThreadPoolExecutor

CONFIGS = "arc_challenge banking77 boolq chaosnli civil_comments clinc150 fever_evidence go_emotions helpsteer2_helpfulness helpsteer2_verbosity ledgar massive measuring_hate_speech mmlu mnli paws sms_spam sst5 strategyqa_closed strategyqa_grounded stsb yelp5".split()
DEFAULT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "jev-bench-data")

def fetch(directory=DEFAULT_DIR):
    os.makedirs(directory, exist_ok=True)
    def one(c):
        path = f"{directory}/{c}.jsonl"
        if not os.path.exists(path):
            tmp = f"{path}.{os.getpid()}.tmp"
            urllib.request.urlretrieve(f"https://huggingface.co/datasets/Praveenrajus/jev-bench/resolve/main/data/{c}/test.jsonl", tmp)
            os.replace(tmp, path)
    with ThreadPoolExecutor(8) as pool:
        list(pool.map(one, CONFIGS))
    return directory

if __name__ == "__main__":
    print(fetch(*sys.argv[1:2]))
