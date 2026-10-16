\l json.k

visits: 0
bench_n: 100000
bench_sum: 0
bench_ms: 0
bench_runs: 0

cr: (_ci 13),(_ci 10)
sq: ,_ci 39

site: jread "kona_site.json"
title: site`title
subtitle: site`subtitle
hero: site`hero
features: site`features
metrics: site`metrics
commands: site`commands
notes: site`notes

resp: {[ct;b] "HTTP/1.1 200 OK",cr,"Content-Type: ",ct,cr,"Content-Length: ",($#b),cr,"Connection: close",cr,cr,b}

state: {.((`title;title;);(`subtitle;subtitle;);(`hero;hero;);(`visits;visits;);(`bench_n;bench_n;);(`bench_sum;bench_sum;);(`bench_ms;bench_ms;);(`bench_runs;bench_runs;))}
api: {resp["application/json";jobj state[]]}
inc: {visits::visits+1; api[]}
rst: {visits::0; bench_sum::0; bench_ms::0; bench_runs::0; api[]}
bench: {t:_T; bench_sum::+/!bench_n; bench_ms::_ 86400000*(_T-t); bench_runs::bench_runs+1; api[]}

css: ":root{color-scheme:dark;--bg:#0a0c0f;--ink:#edf2ef;--muted:#a6b1ad;--line:#2b3337;--panel:#12171a;--panel2:#171d21;--green:#69d39f;--cyan:#70c7e6;--amber:#e7bb60;--red:#ef7d74}*{box-sizing:border-box}html,body{margin:0;min-height:100%;background:var(--bg);color:var(--ink);font:15px/1.45 Inter,ui-sans-serif,system-ui,-apple-system,Segoe UI,sans-serif}body{display:flex;flex-direction:column}.top{height:56px;border-bottom:1px solid var(--line);display:flex;align-items:center;justify-content:space-between;padding:0 max(18px,5vw);background:#0d1114}.brand{font-weight:750;letter-spacing:0}.tag{color:var(--muted);font-size:13px}.shell{width:min(1240px,100%);margin:0 auto;padding:24px max(16px,4vw) 38px}.work{display:grid;grid-template-columns:minmax(0,1.1fr) 410px;gap:18px;align-items:stretch}.pane{border:1px solid var(--line);background:var(--panel);border-radius:8px;overflow:hidden}.headline{padding:26px 28px 20px;background:#11171a;border-bottom:1px solid var(--line)}h1{font-size:44px;line-height:1.02;margin:0 0 12px;letter-spacing:0}.lead{max-width:780px;color:#c7d0cc;font-size:18px;margin:0}.actions{display:flex;gap:10px;flex-wrap:wrap;padding:18px 28px;border-bottom:1px solid var(--line)}button,.btn{height:38px;border-radius:6px;border:1px solid var(--line);background:#1a2225;color:var(--ink);padding:0 13px;font:inherit;cursor:pointer;text-decoration:none;display:inline-flex;align-items:center}button.primary{background:var(--green);border-color:var(--green);color:#07100c;font-weight:750}.stats{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));border-bottom:1px solid var(--line)}.stat{padding:18px 20px;border-right:1px solid var(--line);min-height:112px}.stat:last-child{border-right:0}.num{font-size:30px;line-height:1;margin-bottom:8px;color:var(--green);font-variant-numeric:tabular-nums}.label{color:var(--muted);font-size:13px}.grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:1px;background:var(--line);padding:1px}.tile{background:var(--panel2);padding:20px;min-height:190px}.tile h2,.console h2{font-size:14px;text-transform:uppercase;color:#cbd5d0;letter-spacing:.08em;margin:0 0 14px}.tile ul{margin:0;padding-left:18px;color:#d5ddd9}.tile li{margin:8px 0}.tile code{display:block;margin:8px 0;padding:10px;background:#0c1012;border:1px solid var(--line);border-radius:6px;color:#e6ebe8;overflow:auto}.console{background:#080b0d;color:#dce7e2;height:100%;padding:18px;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.term{min-height:356px;border:1px solid #263036;background:#050708;padding:14px;overflow:hidden}.line{margin:0 0 8px;white-space:pre-wrap}.p{color:var(--amber)}.ok{color:var(--green)}.cy{color:var(--cyan)}.heat{display:grid;grid-template-columns:repeat(16,1fr);gap:4px;margin-top:16px}.cell{height:18px;border-radius:3px;background:#243038}.cell:nth-child(3n){background:#23546a}.cell:nth-child(4n){background:#48612d}.cell:nth-child(7n){background:#704f26}.api{max-height:104px;overflow:auto;color:#b6c2bd}.foot{display:flex;justify-content:space-between;gap:18px;color:var(--muted);padding:18px 2px 0;font-size:13px}@media(max-width:980px){.work{grid-template-columns:1fr}.stats,.grid{grid-template-columns:1fr}.stat{border-right:0;border-bottom:1px solid var(--line)}h1{font-size:36px}}"

feature_html: "<li>",features[0],"</li><li>",features[1],"</li><li>",features[2],"</li>"
metric_html: "<li>",metrics[0],"</li><li>",metrics[1],"</li><li>",metrics[2],"</li>"
command_html: "<code>",commands[0],"</code><code>",commands[1],"</code><code>",commands[2],"</code>"
note_html: "<li>",notes[0],"</li><li>",notes[1],"</li><li>",notes[2],"</li>"
heat: "<div class=heat><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i><i class=cell></i></div>"

mkbody: {seed:jobj state[]; "<!doctype html><html><head><meta charset=utf-8><meta name=viewport content=",sq,"width=device-width,initial-scale=1",sq,"><title>",title,"</title><style>",css,"</style></head><body><header class=top><div class=brand>",title,"</div><div class=tag>",subtitle,"</div></header><main class=shell><section class=work><div class=pane><div class=headline><h1>",title,"</h1><p class=lead>",hero,"</p></div><div class=actions><button class=primary id=bench>Run vector bench</button><button id=inc>Record hit</button><button id=reset>Reset</button><a class=btn href=/api>JSON</a></div><div class=stats><div class=stat><div class=num id=visits>0</div><div class=label>requests through K state</div></div><div class=stat><div class=num id=runs>0</div><div class=label>bench runs</div></div><div class=stat><div class=num id=ms>0</div><div class=label>last bench ms</div></div><div class=stat><div class=num id=sum>0</div><div class=label>sum +/!100000</div></div></div><div class=grid><div class=tile><h2>Runtime Shape</h2><ul>",feature_html,"</ul></div><div class=tile><h2>Hot Path</h2><ul>",metric_html,"</ul></div><div class=tile><h2>Launch</h2>",command_html,"</div></div></div><aside class=pane><div class=console><h2>Kona Trace</h2><div class=term><p class=line><span class=p>k)</span> \\l json.k</p><p class=line><span class=p>k)</span> site:jread ",sq,"kona_site.json",sq,"</p><p class=line><span class=p>k)</span> bench: +/!bench_n</p><p class=line><span class=ok>ready</span> request worker -> .m.h</p><p class=line><span class=cy>api</span></p><pre class=api id=apiout>",seed,"</pre>",heat,"</div></div></aside></section><div class=foot><span>site_web.k renders this page</span><span>json.k parses the config</span></div></main><script>var seed=",seed,";function q(x){return document.getElementById(x)}function draw(d){q(",sq,"visits",sq,").textContent=d.visits;q(",sq,"runs",sq,").textContent=d.bench_runs;q(",sq,"ms",sq,").textContent=d.bench_ms;q(",sq,"sum",sq,").textContent=d.bench_sum;q(",sq,"apiout",sq,").textContent=JSON.stringify(d,null,2)}function hit(p){fetch(p).then(function(r){return r.json()}).then(draw)}q(",sq,"bench",sq,").onclick=function(){hit(",sq,"/bench",sq,")};q(",sq,"inc",sq,").onclick=function(){hit(",sq,"/inc",sq,")};q(",sq,"reset",sq,").onclick=function(){hit(",sq,"/reset",sq,")};draw(seed);</script></body></html>"}
page: {visits::visits+1; resp["text/html; charset=utf-8";mkbody[]]}

.m.h: {[x] :[("a"=x[5])&("p"=x[6])&("i"=x[7]); api[]; :[("i"=x[5])&("n"=x[6])&("c"=x[7]); inc[]; :[("r"=x[5])&("e"=x[6])&("s"=x[7])&("e"=x[8])&("t"=x[9]); rst[]; :[("b"=x[5])&("e"=x[6])&("n"=x[7]); bench[]; page[]]]]]}

`0: "rust kona site loaded: use -h PORT"
