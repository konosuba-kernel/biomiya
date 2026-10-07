#include "runtime.h"
void __biomiya_trap_entry(BiomiyaTrapFrame*f) { (void)f; __asm__ volatile(".word 0"); __builtin_unreachable(); }
int __biomiya_test_entry(void) { __asm__ volatile("ecall");return 0; }
