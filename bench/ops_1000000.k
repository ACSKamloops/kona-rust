/ Made by Claude
/ bullet storm maths, one operation at a time, n = 1000000
bx: 0.8*1000000 _draw 1000
bvx: 0.003*1000000 _draw 1000
by: 0.6*1000000 _draw 1000
bvy: 0.003*1000000 _draw 1000
\t bx+bvx
\t (bx-400.0)*(bx-400.0)
\t 100.0>((bx-400.0)*(bx-400.0))+((by-500.0)*(by-500.0))
\t &(bx<1.0)|(bx>815.0)
\t +/bx
\t _cos bx
\t @[bx;!1000;:;1000#408.0]
\t bx*bvx
/ Made by Claude
