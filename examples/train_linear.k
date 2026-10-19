/ Gradient descent, one statement per line: learn y = 3x + 2 from 2000 random points.
/ Run:  kona examples/train_linear.k
x: 2000 _draw 0
y: (3*x)+2
w: 0.0
b: 0.0
lr: 0.5
n: 20000
i: 0
while[i<n; p:(w*x)+b; e:p-y; w:w-lr*(+/e*x)%#x; b:b-lr*(+/e)%#x; i:i+1]
`0: "steps: ",$i
`0: "w = ",$w
`0: "b = ",$b
`0: "loss = ",$(+/e*e)%#x
