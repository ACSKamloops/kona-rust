\l json.k

cnt: 0
cr: (_ci 13),(_ci 10)
sq: ,_ci 39

resp: {[ct;b] "HTTP/1.0 200 OK",cr,"Content-Type: ",ct,cr,"Content-Length: ",($#b),cr,"Connection: close",cr,cr,b}

state: {.((`count;cnt;);(`kind;"proof";))}

api: {resp["application/json";jobj state[]]}
inc: {cnt::cnt+1; api[]}
rst: {cnt::0; api[]}

css: "body{font-family:monospace;background:#101418;color:#e6f0f2;padding:24px}h1{margin:0 0 12px;color:#7df9c1}.row{display:flex;gap:8px;margin:12px 0}button{padding:8px 12px;border:0;border-radius:6px;background:#7df9c1;color:#07110d;font-weight:bold;cursor:pointer}#count{font-size:48px;margin:8px 0}.note{color:#8ca2a8}"

body: "<!DOCTYPE html><html><head><meta charset=utf-8><title>Kona Proof</title><style>",css,"</style></head><body><h1>kona web proof</h1><div id=count>0</div><div class=row><button id=inc>inc</button><button id=reset>reset</button><button id=refresh>refresh</button></div><div class=note>html from kona, state from kona, routes from kona</div><script>function draw(d){document.getElementById(",sq,"count",sq,").textContent=d.count}function hit(p){fetch(p).then(function(r){return r.json()}).then(draw)}document.getElementById(",sq,"inc",sq,").onclick=function(){hit(",sq,"/inc",sq,")};document.getElementById(",sq,"reset",sq,").onclick=function(){hit(",sq,"/reset",sq,")};document.getElementById(",sq,"refresh",sq,").onclick=function(){hit(",sq,"/api",sq,")};hit(",sq,"/api",sq,");</script></body></html>"

page: {resp["text/html";body]}

.m.h: {[x] :[4<#x; :[("a"=x[5])&("p"=x[6])&("i"=x[7]); api[]; :[("i"=x[5])&("n"=x[6])&("c"=x[7]); inc[]; :[("r"=x[5])&("e"=x[6])&("s"=x[7])&("e"=x[8])&("t"=x[9]); rst[]; page[]]]]; page[]]}

`0: "kona proof web app: listening"
