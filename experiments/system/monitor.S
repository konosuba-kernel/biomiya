/* Test platform only: two harts, M-mode service boundary, S-mode guest.
 * ELF bytes remain unchanged; only PT_LOAD segments are placed in RAM.
 * low 64 MiB -> physical 0x81000000; RAM identity mapping holds the stack.
 * Host clang/lld build this counted platform dependency, never guest code.
 */
.option norvc
.include "guest.inc"
.section .text.boot
.globl _start
_start:
    csrr t0, mhartid
    bnez t0, worker
    li sp, 0x800f0000
    la t0, trap
    csrw mtvec, t0
    /* QEMU resets RAM to zero; tables live in monitor BSS. */
    la t0, root
    la t1, lower
    srli t1, t1, 12
    slli t1, t1, 10
    ori t1, t1, 1
    sd t1, 0(t0)
    li t1, 0x200000cf
    sd t1, 16(t0)             /* 1 GiB identity map at 0x80000000 */
    la t0, lower
    li t1, 0x204000cf
    li t2, 32
1:
    sd t1, 0(t0)
    addi t0, t0, 8
    li t3, 0x80000
    add t1, t1, t3
    addi t2, t2, -1
    bnez t2, 1b
    /* Give supervisor access to physical memory through its mappings. */
    li t0, -1
    csrw pmpaddr0, t0
    li t0, 0x1f
    csrw pmpcfg0, t0
    la t0, root
    srli t0, t0, 12
    li t1, 0x8000000000000000
    or t0, t0, t1
    csrw satp, t0
    sfence.vma
    li t0, 0x800
    csrw mstatus, t0
    li t0, GUEST_ENTRY
    csrw mepc, t0
    li sp, 0x80600000
    mret

/* Hart 1 consumes requests with acquire, publishes response with release.
 * This is actual RV64A communication across two QEMU harts, not volatile
 * ordinary loads treated as synchronization. Only hart 0 submits requests.
 */
worker:
    la t0, mailbox
2:
    amoswap.w.aq t1, zero, (t0)
    beqz t1, 2b
    ld t1, 8(t0)
    addi t1, t1, 17
    sd t1, 16(t0)
    addi t2, t0, 4
    li t1, 1
    amoswap.w.rl zero, t1, (t2)
    j 2b

/* S ecall follows C caller-save registers; s0-s11, sp, ra are preserved.
 * 1024: UART byte; 1025: synchronous mailbox; 1026: install 2 MiB PTE.
 * 1027: expose actual installed PTE. 93: process-style completion.
 */
trap:
    csrr t0, mcause
    li t1, 9
    bne t0, t1, fault
    li t0, 93
    beq a7, t0, finish
    li t0, 1024
    beq a7, t0, uart
    li t0, 1025
    beq a7, t0, exchange
    li t0, 1026
    beq a7, t0, install
    li t0, 1027
    beq a7, t0, inspect
    j fault
uart:
    li t0, 0x10000000
3:
    lbu t1, 5(t0)
    andi t1, t1, 32
    beqz t1, 3b
    fence iorw, iorw
    sb a0, 0(t0)
    fence iorw, iorw
    j resume
exchange:
    la t0, mailbox
    sd a0, 8(t0)
    li t1, 1
    amoswap.w.rl zero, t1, (t0)
    addi t2, t0, 4
4:
    amoadd.w.aq t1, zero, (t2)
    beqz t1, 4b
    ld a0, 16(t0)
    sw zero, 0(t2)
    j resume
install:
    /* Only reserved slots [32,128) can change; enforce an RW/AD leaf.
     * The payload chooses the encoded physical page number itself.
     */
    li t0, 32
    bltu a0, t0, fault
    li t0, 128
    bgeu a0, t0, fault
    /* This platform enables neither Svnapot nor Svpbmt: bits 63:54 are reserved. */
    srli t0, a1, 54
    bnez t0, fault
    andi t0, a1, 1023
    li t1, 199
    bne t0, t1, fault
    srli t0, a1, 10
    andi t1, t0, 511
    bnez t1, fault
    /* Compare the PPN itself: shifting it left could discard forbidden high bits. */
    li t1, 0x85000
    bltu t0, t1, fault
    li t1, 0x87800
    bgeu t0, t1, fault
    la t0, lower
    slli t1, a0, 3
    add t0, t0, t1
    sd a1, 0(t0)
    sfence.vma
    mv a0, a1
    j resume
inspect:
    li t0, 512
    bgeu a0, t0, fault
    la t0, lower
    slli t1, a0, 3
    add t0, t0, t1
    ld a0, 0(t0)
resume:
    csrr t0, mepc
    addi t0, t0, 4
    csrw mepc, t0
    mret
fault:
    li a0, 250
finish:
    /* QEMU virt's SiFive test device: low 0x5555 succeeds, 0x3333 fails. */
    beqz a0, success
    slli a0, a0, 16
    li t0, 0x3333
    or a0, a0, t0
    j shutdown
success:
    li a0, 0x5555
shutdown:
    li t0, 0x100000
    fence iorw, iorw
    sw a0, 0(t0)
5:
    wfi
    j 5b
.section .bss
.balign 4096
root:
    .space 4096
lower:
    .space 4096
.balign 64
mailbox:
    .space 64
