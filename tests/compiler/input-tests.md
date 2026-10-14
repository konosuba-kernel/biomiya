# Q input and lexer acceptance

`tools/check_q_input.py --compiler <compiler.elf> --record-dir <fresh-directory>` compiles the actual Q `model`, `lex`, `input`, `runtime/linux`, and `runtime/bytes` modules into RV64 probe executables. Compilation and execution both run under QEMU. It does not invoke the Rust compiler or substitute an interpreter. Use E3 for Q fixed-point acceptance; E0 runs validate only the migrated fixture and runner.

The runner preserves these four identities from `tests/b3_input.rs`:

| Original test | Q coverage |
| --- | --- |
| `target_lexer_decodes_numbers_strings_ranges_and_nested_comments` | `input.vt::tokens`: nested comments, number suffix/value/span, identifier, decoded NUL escape, operator and EOF. |
| `target_input_keeps_manifest_snapshots_and_handles_short_io` | Source and empty-file snapshots round-trip; normal and seven-byte read/write requests; existing output remains unchanged. An additional injected write failure after seven bytes verifies partial-output cleanup. |
| `target_rejects_bad_numeric_tokens_utf8_and_unsupported_identifiers` | All ten original byte sequences with the original diagnostic code, source identity and byte offset. No output is created. |
| `target_manifest_limits_and_source_aliases_preserve_inputs` | Duplicate entry, parent path, assembly/source alias, wrong profile, unknown key, missing input, output/source alias and the 4 MiB source limit. Inputs remain intact and rejected output is absent. |

`input-short.s0` clamps read/write requests to seven bytes. `input-write-failure.s0` writes seven bytes of the special 32-byte input, then returns `-EIO` on the next output write; stderr remains available. These are test-only wrappers around a copied runtime syscall adapter. Production files are never edited.

The record directory retains the exact compiler image and its hash, every source input and its hash, generated manifests and assembly wrappers, probe ELF hashes, per-case stdout/stderr/status, and `verification.json`. Build outputs and test directories are retained for independent review. A failed run writes `passed: false`; do not reuse its record directory.
