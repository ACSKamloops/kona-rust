/ Made by Claude - first GPU test from K: upload, run a kernel, download, compare with the CPU answer
n: 1000000
a: 0.5*!n
b: 0.25*!n
ha: _gup a
hb: _gup b
hc: _gup n#0.0
k: _gk "add.wgsl"
r: _grun (k;(ha;hb;hc);,2.0;n)
c: _gdown hc
cpu: (a+b)*2.0
`0: ("handles, kernel, threads: "),(5: (ha;hb;hc;k;r)),"\n"
`0: ("first values: "),(5: 5#c),"\n"
`0: ("biggest difference from the CPU: "),(5: |/_abs c-cpu),"\n"
`0: ("relative to the values: "),(5: (|/_abs c-cpu)%|/cpu),"\n"
e1: @[_gk;"broken.wgsl";:]
`0: ("broken kernel -> "),(5: e1),"\n"
e2: @[_gdown;99;:]
`0: ("missing buffer -> "),(5: e2),"\n"
e3: @[_grun;(k;(ha;hb);,2.0;n);:]
`0: ("too few buffers -> "),(5: e3),"\n"
hb2: _gup (hb;b+1.0)
r: _grun (k;(ha;hb;hc);,2.0;n)
`0: ("after refilling b: "),(5: 3#_gdown hc),"\n"
`0: "still alive\n"
/ Made by Claude
