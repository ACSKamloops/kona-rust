/ Made by Claude - race: the same C++ functions through door 2 (built into kona.exe) and door 3 (next door, socket)
`0: "A tiny calls x10000: K built-in, door 2, door 3\n"
\t do[10000; _sqrt 2.0]
\t do[10000; _sys(2;0;2.0)]
\t do[10000; _sys(3;0;2.0)]
p3: 1.0*!1000
p5: 1.0*!100000
p6: 1.0*!1000000
p7: 1.0*!4000000
`0: "B move 1k x1000: K, door 2, door 3\n"
\t do[1000; p3+p3*0.016]
\t do[1000; _sys(2;2;(p3;p3;0.016))]
\t do[1000; _sys(3;2;(p3;p3;0.016))]
`0: "C move 100k x100: K, door 2, door 3\n"
\t do[100; p5+p5*0.016]
\t do[100; _sys(2;2;(p5;p5;0.016))]
\t do[100; _sys(3;2;(p5;p5;0.016))]
`0: "D move 1M x20: K, door 2, door 3\n"
\t do[20; p6+p6*0.016]
\t do[20; _sys(2;2;(p6;p6;0.016))]
\t do[20; _sys(3;2;(p6;p6;0.016))]
`0: "E move 4M x5: K, door 2, door 3\n"
\t do[5; p7+p7*0.016]
\t do[5; _sys(2;2;(p7;p7;0.016))]
\t do[5; _sys(3;2;(p7;p7;0.016))]
w2: _sys(2;3;1000000)
w3: _sys(3;3;1000000)
`0: "F world of 1M stays in C++, step x100: door 2, door 3\n"
\t do[100; _sys(2;4;(w2;0.016))]
\t do[100; _sys(3;4;(w3;0.016))]
`0: "G read 1M positions back x10: door 2, door 3\n"
\t do[10; _sys(2;5;w2)]
\t do[10; _sys(3;5;w3)]
`0: ("worlds still match: "),(5: (_sys(2;5;w2))~_sys(3;5;w3)),"\n"
/ Made by Claude
