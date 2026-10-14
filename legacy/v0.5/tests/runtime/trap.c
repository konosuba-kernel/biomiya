#include "runtime.h"
static int count;
static void handler(BiomiyaTrapFrame*f) { if(f->scause!=9 || (f->sstatus&2) || f->x[0]) __biomiya_platform_fault(); f->sepc+=4; count++; }
void __biomiya_trap_entry(BiomiyaTrapFrame*f) { __biomiya_dispatch(f,handler,0); }
int __biomiya_test_entry(void) { for(int i=0;i<31;i++) __asm__ volatile("ecall":::"memory"); return count; }
