# RV64 atomic lowering

The target is RV64IMA with Zicsr/Zifencei and LP64. The integer atomic mapping
is pinned to [riscv-atomic.adoc at 48969b7995b3ae1582f230dcce32a12d443d1a7a](https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/48969b7995b3ae1582f230dcce32a12d443d1a7a/riscv-atomic.adoc).
All source orders remain in HIR/MIR; this first emitter strengthens every
operation to sequential consistency, as permitted by reference 22/RV64 R4.

| Operation | Emitted mapping |
| --- | --- |
| Load | `fence rw,rw; lw[u]/ld; fence r,rw` |
| Store | `fence rw,w; sw/sd; fence rw,rw` |
| Native exchange/add/and/or | `amo*.w/d.aqrl` |
| Subtract | Negate operand, then `amoadd.w/d.aqrl` |
| Narrow masked arithmetic/bitwise RMW | `lr.w.aqrl; op; truncate to payload; sc.w.rl; bnez retry` |
| Strong CAS | Leading `fence rw,rw`; `lr.w/d.aqrl`; compare; `sc.w/d.rl`; retry SC reservation failure |

`Atomic<u8>`/`Atomic<u16>` own independent four-byte aligned storage. Stores
and desired CAS values are zero-extended payloads. Narrow arithmetic masks
before SC, keeping upper bits zero, and does not read or write neighboring
storage. Failed comparison returns Err(actual); failed SC retries without
reporting a spurious Err. LR/SC retry sequences contain no calls, ordinary
loads/stores, fences or intervening backward branches, and stay below the
16-instruction constrained-loop limit from the [ratified A-extension LR/SC rules](https://docs.riscv.org/reference/isa/v20240411/unpriv/a-st-ext.html).
CAS uses the same address/width in LR and SC. Its conditional mismatch branch
exits the loop; the SC failure branch returns to LR. Ordinary platform RAM
must provide the ISA eventuality property; this is a platform contract. The leading CAS fence also preserves
the strengthened failure ordering.

`tools/test_integration.py --group atomics` builds actual language sources
and retains production assembly/ELF hashes and QEMU output. Its all-width
case covers every operation on u8/u16/u32/u64/usize plus pointer load/store/
exchange/CAS and checks sentinel fields adjacent to narrow atomic storage.
Its two-hart case checks release/acquire publication and contending AMO/CAS
counts. The reservation-loss case first executes the unmodified compiler ELF,
then creates a clearly marked test-only ELF: a one-shot dummy SC writes the
same expected zero, consuming the reservation immediately before the original
SC. It asserts that dummy SC succeeds and the original SC fails; the existing
production retry must subsequently return Ok(0) and store 42. The original
loop shape is checked separately before instrumentation. No instrumentation
is enabled in the compiler or normal runtime.

These execution checks support this specific mapping and platform. They do
not constitute an exhaustive memory-model proof or an Extend concurrency
verification result.

## Byte names and assembly artifacts

`@export` and `@section` strings retain their original nonempty, non-NUL bytes.
Invalid UTF-8 uses a reversible internal identity; it is never replaced by U+FFFD.
Names that LLVM assembly cannot quote faithfully use deterministic ASCII aliases.
Every explicit section uses an alias so user section names cannot collide with
other section aliases. After assembly, `linkage::restore_object_names` extends
ELF string tables and updates symbol/section name indices. Relocation indices,
code, data and section flags remain intact. Object and ELF outputs contain the
original byte names.

Assembly output requiring aliases includes `OUTPUT.s.linkage.json`. Assemble
it through the CLI, which automatically discovers that sidecar:

```
biomiya assemble --diagnostics json -o output.o OUTPUT.s
```

A moved sidecar can be passed with `--linkage PATH`. A plain assembler invocation
cannot restore aliased names; the CLI rejects a required missing sidecar.
The supplemental `byte-exact-export-and-section` case checks linked ELF, direct
object output, and assembly reassembled through the CLI. It inspects actual ELF
symbol/section references for distinct invalid UTF-8 names, quotes, backslashes,
newlines, valid Unicode and high section-name bytes, and executes the linked
program in QEMU. A static pointer relocation also references an exported byte name.

## Emitter resource boundary

The reusable emitter's direct aggregate `Binary ==/!=` fallback stops after
100,000 recursive type/component visits with `I_LIMIT`. The count includes
aggregate nodes and code for every enum variant. Normal source lowering uses
loop-based aggregate equality and avoids loops for zero-sized elements, so this
fallback limit applies to direct MIR clients. Copy and zero operations use short
runtime loops and have no fixed byte-count expansion cap. Stack sizes and offsets
use checked 64-bit target arithmetic; the selected platform must supply sufficient
stack/storage, with further stack proofs reserved for optional analysis.

ELF name restoration bounds the complete section-header range before allocating
its metadata vector. Malformed extended counts and truncated tables return
`E_LINK`, rather than triggering a host capacity panic.
