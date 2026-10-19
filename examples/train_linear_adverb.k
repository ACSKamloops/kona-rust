/ The same training loop the K way: state carried by an over adverb, {cond} step/ state.
/ The 2026-10-15 build stopped this at step 100000 with `limit` (MAX_ADVERB_STEPS in adverbs.rs);
/ the current build runs it until the condition says stop, as C Kona does.
/ Run:  kona examples/train_linear_adverb.k

x: 500 _draw 0
y: (3*x)+2
lr: 0.5
n: 150000

/ state is (w;b;i); one step returns the next state
step: {[s] w:s[0]; b:s[1]; i:s[2]; p:(w*x)+b; e:p-y; (w-lr*(+/e*x)%#x; b-lr*(+/e)%#x; i+1)}
going: {x[2]<n}

r: going step/ (0.0;0.0;0)

`0: "steps: ",$r[2]
`0: "w = ",$r[0]
`0: "b = ",$r[1]
