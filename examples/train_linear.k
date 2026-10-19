/ Gradient descent in a while loop: learn y = 3x + 2 from 2000 random points.
/ Run:  kona examples/train_linear.k

x: 2000 _draw 0            / inputs in [0,1)
y: (3*x)+2                 / the line to learn
w: 0.0                     / weight
b: 0.0                     / bias
lr: 0.5
n: 20000
i: 0
while[i<n
  p: (w*x)+b               / prediction
  e: p-y                   / error
  w: w-lr*(+/e*x)%#x       / dL/dw, mean over the batch
  b: b-lr*(+/e)%#x         / dL/db
  i: i+1]

`0: "steps: ",$i
`0: "w = ",$w
`0: "b = ",$b
`0: "loss = ",$(+/e*e)%#x
