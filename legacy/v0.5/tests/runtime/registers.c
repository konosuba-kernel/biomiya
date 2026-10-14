#include "runtime.h"
extern int register_probe(void); extern char register_resume[];
static void handler(BiomiyaTrapFrame*f) { if(f->sepc+4!=(uintptr_t)register_resume||f->scause!=9||(f->sstatus&2)||f->x[0]||f->x[2]%16) __biomiya_platform_fault();for(unsigned i=1;i<32;i++)if(i!=2){if(f->x[i]!=i*17)__biomiya_platform_fault();f->x[i]=i*19;}f->sepc+=4; }
void __biomiya_trap_entry(BiomiyaTrapFrame*f) { __biomiya_dispatch(f,handler,0); }
int check_registers(uintptr_t*x) {for(unsigned i=1;i<32;i++)if(i!=2&&x[i]!=i*19)return -1;return 31;}
int __biomiya_test_entry(void) {return register_probe();}
