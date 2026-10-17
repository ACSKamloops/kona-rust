/ --- constants ---
dq: ,_ci 34
lsb: ,_ci 91
rsb: ,_ci 93
lcb: ,_ci 123
rcb: ,_ci 125
cm: ,_ci 44
cl: ,_ci 58
eval: (.:)

/ --- encoder ---
q: {dq,x,dq}
jv: {t:4:x; :[t=1; $x; t=2; $x; t=-3; q x; t=4; q ($x); t=-1; "[",(","/({$x}' x)),"]"; t=-2; "[",(","/({$x}' x)),"]"; t=6; "null"; t=0; jarr x; $x]}
jarr: {"[",(","/jv' x),"]"}
jp: {(q ($x)),":",(jv y)}
jobj: {k: !x; v: :[1=#k; ,(x[*k]); x[]]; pairs: {jp[x;y]}'[k;v]; lcb,(","/pairs),rcb}

/ --- decoder internals ---
unbrace: {1_(-1)_ x}
trimc: {:[0=#x; x; cm[0]=x[(-1)+#x]; (-1)_ x; x]}
depths: {n:#x; d: n#0; dep: 0; inq: 0; i: 0; while[i<n; c: x[i]; :[c=dq[0]; inq: ~inq; (~inq)&(c=lsb[0])|(c=lcb[0]); dep: dep+1; (~inq)&(c=rsb[0])|(c=rcb[0]); dep: dep-1; 0]; d[i]: :[inq;-1;dep]; i: i+1]; d}
dsplit: {d: depths x; cpos: &(x=cm[0])&(d=y); trimc' (0,(1+cpos))_ x}

/ parse a JSON value: detect type by first char, recurse as needed
/ blanks around a value are not part of it ({"a": 1} carries " 1")
trim: {x: (),x; x: (+/&\x=" ") _ x; (-(+/&\|x=" ")) _ x}
/ a JSON number: optional -, digits with at most one point, then optionally e or E, optional -, digits.
/ Only that reaches eval, so a JSON file cannot run K through its unquoted values (it used to:
/ {"a":6:"/etc/passwd"} read the file, and a character-set test still let {"a":e} fetch a global).
/ (),x makes a one-character value a list; f[..] brackets because `f :[..]` inside a lambda reads as f: here.
isdig: {(0<#x)&&/x _lin "0123456789"}
neg: {:[0<#x; x[0]="-"; 0]}
ismant: {x: (),x; (2>+/x=".")&isdig[x _dv "."]}
isexp: {x: (),x; isdig[:[neg x; 1_ x; x]]}
isnum: {x: (),x; x: :[neg x; 1_ x; x]; e: &x _lin "eE"; :[1<#e; 0; 0=#e; ismant x; ismant[e[0]#x]&isexp[(1+e[0])_ x]]}
pval: {x: trim x; :[0=#x; _n; x[0]=lcb[0]; pobj x; x[0]=lsb[0]; parr x; x[0]=dq[0]; unbrace x; x~"null"; _n; x~"true"; 1; x~"false"; 0; isnum x; eval x; x]}

/ parse array: strip brackets, split on commas at depth 0, parse each
parr: {inner: unbrace x; :[0=#inner; !0; pval' dsplit[inner;0]]}

/ parse object: strip braces, split pairs at depth 0, parse each pair
pobj: {inner: unbrace x; pairs: dsplit[inner;0]; entries: ppair' pairs; eval entries}

/ parse one "key":value pair — find colon after closing quote of key
ppair: {qp: _ss[x;dq]; k: `$((1+qp[0])_((qp[1])#x)); vs: (2+qp[1])_ x; v: pval vs; (k;v;)}

/ --- public decoder ---
pjson: {pobj x}

/ --- file I/O ---
jwrite: {(`$x) 0: ,(jobj y)}
jread: {r: *0:(`$x); pjson r}
