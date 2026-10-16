/ Made by Claude - Kona calling Martin Richards' BCPL (his cintsys running kqueens): one at a time vs all at once
`0: ("one call, numbers: "),(5: _sys(4;0;"kqueens 8")),"\n"
`0: ("one call, text: "),_sys(4;1;"kqueens 10")
jobs: 12#,"kqueens 13"
calls: {(4;0;x)}'jobs
warm: _sys(0;0;calls)
`0: "12 x kqueens 13, one at a time:\n"
\t r1: {_sys(4;0;x)}'jobs
`0: "12 x kqueens 13, all at once:\n"
\t r2: _sys(0;0;calls)
`0: ("same answers: "),(5: r1~r2),"   first: ",(5: *r2),"\n"
mix: ("kqueens 10";"kqueens 11";"kqueens 12";"kqueens 13";"kqueens 14")
`0: "sizes 10..14 all at once:\n"
\t r3: _sys(0;0;{(4;0;x)}'mix)
`0: (5: r3),"\n"
`0: "BCPL and C++ in one go: queens 14 + a 1M sum + echo\n"
pp: 1.0*!1000000
\t r4: _sys(0;0;((4;0;"kqueens 14");(2;1;pp);(2;0;7.5)))
`0: (5: r4),"\n"
`0: "still alive\n"
/ Made by Claude
