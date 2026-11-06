import json,sys
d=json.load(sys.stdin)
m=list(d.get("errors") or [])
for lvl,items in (d.get("recoverable_errors") or {}).items():
    m+=["%s: %s" % (lvl,i) for i in items]
print("; ".join(m)[:1500])
