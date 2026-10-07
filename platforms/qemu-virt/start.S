.option norvc
.option norelax
.section .text.start,"ax"
.global _start
_start:
 csrr a0, mhartid
 li t0, 4
 bgeu a0,t0,.Lpark
 la gp,__global_pointer$
 la sp,__boot_stacks
 addi t0,a0,1
 slli t0,t0,17
 add sp,sp,t0
 la tp,__hart_states
 slli t0,a0,16
 add tp,tp,t0
 la t0,.Lm_fault
 csrw mtvec,t0
 bnez a0,.Lwait
 la t0,__bss_start
 la t1,__bss_end
.Lzero:
 bgeu t0,t1,.Lready
 sd zero,0(t0)
 addi t0,t0,8
 j .Lzero
.Lready:
 fence rw,rw
 la t0,__boot_ready
 li t1,1
 sw t1,0(t0)
 j .Lsetup
.Lwait:
 la t0,__boot_ready
 lw t1,0(t0)
 beqz t1,.Lwait
 fence rw,rw
.Lsetup:
 sd zero,0(tp)
 sd zero,8(tp)
 li t0,-1
 csrw pmpaddr0,t0
 li t0,15
 csrw pmpcfg0,t0
 li t0,0xffff
 csrw medeleg,t0
 li t0,0x222
 csrw mideleg,t0
 csrw satp,zero
 csrw sie,zero
 la t0,__biomiya_trap_vector
 csrw stvec,t0
 csrw sscratch,tp
 csrr t0,mstatus
 li t1,0x1800
 not t1,t1
 and t0,t0,t1
 li t1,0x800
 or t0,t0,t1
 csrw mstatus,t0
 la t0,.Ls_start
 csrw mepc,t0
 mret
.Ls_start:
 call __biomiya_runtime_start
.Lpark:
 wfi
 j .Lpark
.Lm_fault:
 li t0,0x10000000
 li t1,66
 sb t1,0(t0)
 li t0,0x10000000
 li t1,73
 sb t1,0(t0)
 li t0,0x10000000
 li t1,79
 sb t1,0(t0)
 li t0,0x10000000
 li t1,77
 sb t1,0(t0)
 li t0,0x10000000
 li t1,73
 sb t1,0(t0)
 li t0,0x10000000
 li t1,89
 sb t1,0(t0)
 li t0,0x10000000
 li t1,65
 sb t1,0(t0)
 li t0,0x10000000
 li t1,58
 sb t1,0(t0)
 li t0,0x10000000
 li t1,70
 sb t1,0(t0)
 li t0,0x10000000
 li t1,65
 sb t1,0(t0)
 li t0,0x10000000
 li t1,85
 sb t1,0(t0)
 li t0,0x10000000
 li t1,76
 sb t1,0(t0)
 li t0,0x10000000
 li t1,84
 sb t1,0(t0)
 li t0,0x10000000
 li t1,10
 sb t1,0(t0)
 li t0,0x100000
 li t1,0x5555
 sw t1,0(t0)
 j .Lpark
.section .data
.align 2
__boot_ready: .word 0
