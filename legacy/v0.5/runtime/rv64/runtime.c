#include "runtime.h"
static void put(char c) { volatile unsigned char *u=(void*)0x10000000; while (!(u[5]&32)) {} u[0]=(unsigned char)c; }
static void text(const char *s) { while (*s) put(*s++); }
static void halt(void) { *(volatile unsigned int*)0x100000=0x5555; for (;;) __asm__ volatile("wfi"); }
void __biomiya_panic(const unsigned char *p,uintptr_t n) { (void)p;(void)n; __asm__ volatile("csrci sstatus,2"); text("BIOMIYA:PANIC\n"); halt(); __builtin_unreachable(); }
void __biomiya_platform_fault(void) { text("BIOMIYA:FAULT\n"); halt(); __builtin_unreachable(); }
__attribute__((weak)) void kernel_main(void) { __biomiya_platform_fault(); }
__attribute__((weak)) int __biomiya_test_entry(void) { kernel_main(); return 0; }
__attribute__((weak)) void __biomiya_secondary_main(uintptr_t hart) { (void)hart; for (;;) __asm__ volatile("wfi"); }
void __biomiya_runtime_start(uintptr_t hart) {
 if (hart) __biomiya_secondary_main(hart);
 int value=__biomiya_test_entry(); text("BIOMIYA:PASS:");
 unsigned int n=(unsigned int)value; if(value<0) { put('-'); n=0u-n; }
 char digits[10]; unsigned i=0; do { digits[i++]=(char)('0'+n%10); n/=10; } while(n); while(i) put(digits[--i]); put('\n'); halt();
}
__attribute__((weak)) void __biomiya_trap_entry(BiomiyaTrapFrame *f) { (void)f; __biomiya_platform_fault(); }
void *memcpy(void *d,const void *s,size_t n) { unsigned char *a=d;const unsigned char*b=s;while(n--)*a++=*b++;return d; }
void *memset(void *d,int c,size_t n) {unsigned char*a=d;while(n--)*a++=(unsigned char)c;return d;}
