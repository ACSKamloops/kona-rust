/ Minimal test
\l retrieval_kernel.k

` 0: "Testing rrf..."
d_ranks: 1 5 10 0N
s_ranks: 3 0N 5 2
fusion: rrf[d_ranks; s_ranks; 60]
` 0: $fusion

` 0: "Testing calibrate..."
cal: calibrate[1 2 3 4 5; 5]
` 0: $cal

` 0: "Testing packing..."
docs: ((`score`token_len`source_id!(0.9; 100; "a"));
       (`score`token_len`source_id!(0.8; 200; "b"));
       (`score`token_len`source_id!(0.7; 50; "a")))
packed: pack_context[docs; 300; 1]
` 0: $packed

` 0: "All tests passed."
\\
