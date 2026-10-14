# Reference RV64 runtime

This is `qemu-virt-core-v1`, using repository M-mode startup, S-mode bare
addressing (`satp=0`), PMP access, delegated supervisor exceptions and software
interrupts. It does not use OpenSBI. The reference environment observed on
2026-10-06 is Clang/LLD 23.1.1 and QEMU 11.1.1 on Linux x86_64.

Build `platforms/qemu-virt/start.S`, `runtime/rv64/trap.S`,
`runtime/rv64/runtime.c` and generated program assembly as objects with:

```
clang --target=riscv64-unknown-elf -march=rv64ima_zicsr_zifencei \
  -mabi=lp64 -mno-relax -mcmodel=medany -ffreestanding -fno-builtin \
  -fno-stack-protector -O1 -Iruntime/rv64 -c INPUT -o OUTPUT
ld.lld -m elf64lriscv --no-relax -T platforms/qemu-virt/link.ld OBJECTS -o kernel.elf
qemu-system-riscv64 -machine virt -accel tcg -cpu rv64 -m 128M -smp 1 \
  -bios none -display none -monitor none -serial stdio -kernel kernel.elf
```

The ELF starts at physical 0x80000000. The fixed virt configuration provides
UART at 0x10000000 and the SiFive test finisher at 0x100000. No paging or
scheduler is installed. Linker segments are RX, R, and RW; bare-mode PMP gives
S-mode access to all memory, so segment permissions are ELF declarations, not
hardware-enforced protection. Objects may request alignment through 4096.
There are four supported harts, each with a 128 KiB ordinary stack and a 64 KiB
state/trap-stack region. Hart zero initializes BSS and release-publishes boot
readiness. Each hart establishes gp, tp (its state pointer), and sscratch.
Secondary harts enter weak `__biomiya_secondary_main(hart)` and ordinarily park.

The compiler test adapter exports `__biomiya_test_entry(void) -> i32`, calling
its resolved ordinary biomiya test function. The weak runtime fallback calls
exported LP64 C `kernel_main(void)` and reports zero if it returns. A missing
kernel_main enters a platform fault. A chosen special entry exports
`__biomiya_trap_entry(frame) -> !`; the weak fallback reports a platform fault.
Runtime panic/dispatch symbols are `__biomiya_panic` and `__biomiya_dispatch`.
Memcopy/memset helpers are supplied without allocation or unwinding.

UART terminal records are exactly `BIOMIYA:PASS:<signed-i32>`,
`BIOMIYA:PANIC`, or `BIOMIYA:FAULT`. All observed terminal records stop QEMU
using the finisher. PASS values including nonzero and negative are data;
the host compares the expected value. No record, multiple records, process
failure, and host timeout must remain distinguishable infrastructure failures.
Panic does not inspect message bytes, unwind, run defer, allocate or acquire a
log lock. The trap entry saves all integer registers to a 288-byte public
TrapFrame at state+16; x2 is the interrupted stack pointer. During entry and
handlers SIE is off, gp/tp and a separate stack are reestablished. Dispatch
checks current-frame identity and transfer kind against scause, calls the handler, checks the returning PC within the linked image and the aligned stack within that hart's ordinary stack, restores
sepc/sstatus/registers, releases the active record, and SRETs. Returning code
must satisfy the platform's legal instruction address (the image range guard is coarse and does not prove an instruction boundary or validity), S-mode privilege and
accessible aligned stack contracts. Hardware fault snapshots are not written
back. A synchronous nested fault is detected before writing the active frame;
it uses a stackless, lock-free panic path, preserving active-frame storage.
There is no support for scheduling, sleeping, enabling SIE in a handler,
changing mappings, or returning into another privilege mode.

Run `python3 tools/test_runtime.py` (`--json` retains command/result details).
These are independent C/assembly platform probes, not compiler conformance
results. They cover initialized data/BSS/S-mode, signed reporting, panic,
platform fault, repeated synchronous trap recovery, a supervisor software IRQ,
register save/restore including interrupted gp/tp, nested-fault panic, two-hart
release/acquire publication and 10,000 contending RMW operations, strong CAS,
and 32-bit isolated narrow-storage operations, host timeout classification, and real UART scratch-register MMIO. MMIO checks
four accesses at 0x10000007, plus 64-bit and 32-bit read/write pairs at
CLINT mtimecmp 0x02004000 and 16-bit CFI query commands/reads from the real
virt.flash0 device at 0x20000000. CFI query mode ensures reads go through
the device instead of ROMD RAM. Trace checks order, values and widths through QEMU
`memory_region_ops_read/write`, plus two decoded `fence iorw,iorw` sites.
QEMU instruction logging proves the fixture's decoded instruction sites; it
is not a complete device-ordering or memory-model proof.
`tools/test_integration.py` separately runs compiler-generated cleanup,
MMIO widths, multihart atomics and strong-CAS reservation-loss injection; see
`src/target/README.md` for the chosen mapping and its review basis. Finite
machine runs support the acceptance evidence without establishing a formal
proof of all executions of the atomic memory model.

Primary references: [QEMU virt](https://www.qemu.org/docs/master/system/riscv/virt.html),
[RISC-V supervisor ISA](https://docs.riscv.org/reference/isa/priv/supervisor.html),
and the project's pinned psABI revision in `docs/architecture-rv64.md`.
