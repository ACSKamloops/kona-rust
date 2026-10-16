/ Made by Claude - the _sys door: library 2 (C++ built in) and library 3 (C++ next door) give the same answers as K
pp: 0.5*!1000
pv: 0.25*!1000
`0: ("echo 2 and 3: "),(5: (_sys(2;0;3.5);_sys(3;0;3.5))),"\n"
`0: ("echo a list through 3: "),(5: _sys(3;0;(1 2 3;4.5 6.5))),"\n"
`0: ("sum 2, 3, K: "),(5: (_sys(2;1;pp);_sys(3;1;pp);+/pp)),"\n"
m2: _sys(2;2;(pp;pv;0.016))
m3: _sys(3;2;(pp;pv;0.016))
mk: pp+pv*0.016
`0: ("move, biggest gap 2 vs K and 3 vs K: "),(5: (|/_abs m2-mk;|/_abs m3-mk)),"\n"
h2: _sys(2;3;1000)
h3: _sys(3;3;1000)
s2: _sys(2;4;(h2;0.016))
s3: _sys(3;4;(h3;0.016))
r2: _sys(2;5;h2)
r3: _sys(3;5;h3)
`0: ("world handles and steps: "),(5: (h2;h3;s2;s3)),"\n"
`0: ("worlds match: "),(5: r2~r3),"\n"
`0: ("first x: "),(5: 3#r2@0),"\n"
e2: @[_sys;(2;99;1.0);:]
e3: @[_sys;(3;99;1.0);:]
`0: ("bad function 2: "),(5: e2),"\n"
`0: ("bad function 3: "),(5: e3),"\n"
e4: @[_sys;(2;5;77);:]
`0: ("missing world: "),(5: e4),"\n"
e5: @[_sys;(9;0;1.0);:]
`0: ("no library 9: "),(5: e5),"\n"
gh: _sys(1;0;1.0 2.0 3.0)
`0: ("gpu through the door, handle and back: "),(5: (gh;_sys(1;1;gh))),"\n"
`0: "still alive\n"
/ Made by Claude
