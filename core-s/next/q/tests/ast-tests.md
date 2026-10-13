# AST arena acceptance probes

Run manifests from `biomiya/core-s`. `ast-cases.json` records expected compilation and execution results. Compile `ast-lifecycle.build` once, then execute its output with each listed argument. The two rejection manifests use compiler `check` and never run an output executable. Output paths are create-new: use a fresh directory or rewrite only the manifest output for each compiler generation.

The lifecycle probe verifies `sizeof(Node) == 40` and `sizeof(TypeSyntax) == 24`, persists a header constant and its type across two function arenas, and checks invalid handles after ending a function and after reusing its arena indices. Negative compilation probes reject mixing `TypeSyntaxId` with `NodeId`, and reject external `zero` construction of a private `NodeId`.

Initial evidence: the P3 E0 compiler built the lifecycle fixture and all four runtime expectations passed under `qemu-riscv64 -s 1048576`; both compilation rejection diagnostics matched. Q fixed-point acceptance must compile these tracked inputs again with E3 and replay the same expectations. The initial temporary evidence is `build/q-front-life-results.json`.
