#include "runtime.h"
static int count;
static void handler(BiomiyaTrapFrame*f) { if(f->scause!=((1ULL<<63)|1) || (f->sstatus&2)) __biomiya_platform_fault();__asm__ volatile("csrci sip,2");count++; }
void __biomiya_trap_entry(BiomiyaTrapFrame*f) { __biomiya_dispatch(f,handler,1); }
int __biomiya_test_entry(void) { __asm__ volatile("csrsi sie,2\ncsrsi sip,2\ncsrsi sstatus,2":::"memory");while(!count)__asm__ volatile("nop":::"memory");__asm__ volatile("csrci sstatus,2");return count; }
