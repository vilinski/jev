"""One `jev run` call: used by the own-set runner and the public-sample runner."""
import json, subprocess, time

def call(provider, body, retries=0, timeout=120):
    """Send a System One request body through the jev CLI; returns timing, parsed answers and the usage line."""
    t = time.perf_counter()
    p = subprocess.run(["jev", "run", "-", "-p", provider, "--retries", str(retries)], input=json.dumps(body), capture_output=True, text=True, timeout=timeout)
    dt = time.perf_counter() - t
    usage = next((l[6:].strip() for l in p.stderr.splitlines() if l.startswith("usage:")), None)
    try:
        res = json.loads(p.stdout)
    except ValueError:
        res = None
    return dict(ok=p.returncode == 0 and res is not None, dt=dt, res=res, usage=usage, err=None if p.returncode == 0 else p.stderr[-300:])
