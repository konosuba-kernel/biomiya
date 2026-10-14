#include "runtime.h"
int check_registers(uintptr_t *x) { for(unsigned i=1;i<32;i++)if(i!=2&&x[i]!=i*19)return -1;return 31; }
