#include "runtime.h"
static void handler(BiomiyaTrapFrame*f) {f->sepc=0;}
void __biomiya_trap_entry(BiomiyaTrapFrame*f) {__biomiya_dispatch(f,handler,0);}
int __biomiya_test_entry(void) {__asm__ volatile("ecall");return 0;}
