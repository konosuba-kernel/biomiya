#include "runtime.h"
static int initialized=37; static int zero;
int __biomiya_test_entry(void) { uintptr_t status; __asm__ volatile("csrr %0,sstatus":"=r"(status)); if(initialized!=37||zero||!(status&0x200000000ULL)) return 99; return (-2147483647-1); }
