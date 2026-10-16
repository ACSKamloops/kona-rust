/ Minimal retrieval kernel for Kona
/ Testing RRF only

rrf: {[d_ranks; s_ranks; k]
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

calibrate: {[ranks; max_rank]
  $[max_rank=0; {0.0}'ranks; 1.0 - sdiv[;max_rank]'ranks]
 }

sdiv: {$[y=0; 0.0; x%y]}
