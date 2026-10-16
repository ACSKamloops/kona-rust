nb: 1000000
a: 6.2832*nb _draw 0
sp: 1.5+0.6*(!nb)!5
bx: 816.0*nb _draw 0
by: 624.0*nb _draw 0
bvx: sp*_cos a
bvy: sp*_sin a
\t bx: bx+bvx
\t ddx: bx-408.0
\t ddy: by-500.0
\t hm: 100.0>(ddx*ddx)+(ddy*ddy)
\t out: hm|(bx<0.0)|(bx>816.0)|(by<0.0)|(by>624.0)
\t i: &out
\t k: #i
\t s: sp@i
\t +/hm
\t (!nb)!5
\t #bx
