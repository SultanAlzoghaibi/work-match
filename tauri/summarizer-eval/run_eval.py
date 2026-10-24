import json, sys, urllib.request, re, datetime
from cases import CASES
SCHEMA={"type":"object","properties":{"shareable":{"type":"boolean"},"topic":{"type":"string","maxLength":40},"task_summary":{"type":"string","maxLength":120}},"required":["shareable","topic","task_summary"],"additionalProperties":False}
def build(apps,pages,ctx,ctxfmt):
    p=""
    if ctx and ctxfmt: p+=f"About right now: {ctx}\n\n"
    if apps:
        p+="Apps in front in the last few minutes, most time first:\n"
        for a,t,m in sorted(apps,key=lambda x:-x[2]): p+=f"- {a}: {t} ({m} min)\n"
        p+="\n"
    if pages:
        p+="Pages from the last few minutes, newest first:\n"
        for i,(h,path,title,text) in enumerate(pages): p+=f"\n[11:0{5-i}] {h}{path.rstrip('/')} | {title}\n{text[:800]}\n"
    return p
def ask(sysp,user):
    body={"messages":[{"role":"system","content":sysp},{"role":"user","content":user}],"temperature":0,"response_format":{"type":"json_object","schema":SCHEMA}}
    r=urllib.request.urlopen(urllib.request.Request("http://127.0.0.1:8095/v1/chat/completions",json.dumps(body).encode(),{"Content-Type":"application/json"}))
    o=json.loads(json.loads(r.read())["choices"][0]["message"]["content"])
    bad=any(m in (o["topic"]+o["task_summary"]).lower() for m in ["@","$","http","www.","[email]","[phone]"])
    if not o["shareable"] or bad: return "— not shared"+(" (blocked by filter)" if bad and o["shareable"] else "")
    return f'**{o["topic"]}**: {o["task_summary"]}'
prompts={k:open(f).read() for k,f in [("current",sys.argv[1]),("new",sys.argv[2])]}
out=["| Case | Input (apps · pages · context) | Expected | Current prompt | New prompt |","|---|---|---|---|---|"]
for name,apps,pages,ctx,exp in CASES:
    inp="<br>".join([f"{a}: {t} ({m} min)" for a,t,m in apps]+[f"page {h}" for h,_,_,_ in pages]+([f"context: {ctx}"] if ctx else []))
    cur=ask(prompts["current"],build(apps,pages,ctx,False))
    new=ask(prompts["new"],build(apps,pages,ctx,True))
    out.append(f"| {name} | {inp} | {exp} | {cur} | {new} |"); print(out[-1],flush=True)
open(sys.argv[3],"w").write("\n".join(out)+"\n")
