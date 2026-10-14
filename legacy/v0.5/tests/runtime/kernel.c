#include "runtime.h"
void kernel_main(void) { uintptr_t address;__asm__ volatile("mv %0,tp":"=r"(address));if(!address)__biomiya_platform_fault(); }
