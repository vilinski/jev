import sys
src=open("report.py").read().split("R={n:main")[0]
src=src.replace('P = {"Jev (TypeSafe)":"jev.json","Clef (Cloudflare)":"clef.json"}','P = {"Jev":"jev.json","Clef":"clef.json","OpenAI gpt-4.1-mini":"openai.json"}')
src=src.replace('COL = {"Jev (TypeSafe)":"#2f6fed","Clef (Cloudflare)":"#e8710a"}','COL = {"Jev":"#2f6fed","Clef":"#e8710a","OpenAI gpt-4.1-mini":"#12a36b"}')
exec(src)
R={n:main(json.load(open(f))) for n,f in P.items()}
