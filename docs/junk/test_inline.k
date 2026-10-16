/ Load the test_minimal inline
rrf_simple: {[d_ranks; s_ranks; k]
  n: #d_ranks;
  scores: n#0.0;
  i: 0;
  while[i<n;
    dr: d_ranks[i];
    sr: s_ranks[i];
    $[~dr~0N; scores[i]: scores[i] + 1.0 % k + dr; :()];
    $[~sr~0N; scores[i]: scores[i] + 1.0 % k + sr; :()];
    i: i+1
  ];
  scores
 }

fusion: rrf_simple[1 5 10 0N; 3 0N 5 2; 60]
$fusion
\\
