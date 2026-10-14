#include "runtime.h"
static unsigned flag, done, counter, cas;
static int data;
static struct { unsigned narrow; unsigned neighbor; } storage={0,0x12345678};
void __biomiya_secondary_main(uintptr_t h) { (void)h; data=7; __atomic_store_n(&flag,1,__ATOMIC_RELEASE);for(unsigned i=0;i<5000;i++)__atomic_fetch_add(&counter,1,__ATOMIC_SEQ_CST);__atomic_store_n(&done,1,__ATOMIC_RELEASE);for(;;)__asm__ volatile("wfi"); }
int __biomiya_test_entry(void) {
 while(!__atomic_load_n(&flag,__ATOMIC_ACQUIRE)){} if(data!=7)return -1;
 for(unsigned i=0;i<5000;i++)__atomic_fetch_add(&counter,1,__ATOMIC_SEQ_CST);
 while(!__atomic_load_n(&done,__ATOMIC_ACQUIRE)){}
 if(__atomic_load_n(&counter,__ATOMIC_SEQ_CST)!=10000)return -2;
 unsigned expected=0;if(!__atomic_compare_exchange_n(&cas,&expected,1,0,__ATOMIC_SEQ_CST,__ATOMIC_ACQUIRE)||expected!=0)return -3;
 __atomic_store_n(&storage.narrow,255,__ATOMIC_SEQ_CST);
 if(__atomic_exchange_n(&storage.narrow,65535,__ATOMIC_SEQ_CST)!=255||storage.neighbor!=0x12345678||storage.narrow!=65535)return -4;
 return data;
}
