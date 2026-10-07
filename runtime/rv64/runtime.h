#ifndef BIOMIYA_RUNTIME_H
#define BIOMIYA_RUNTIME_H
#include <stdint.h>
#include <stddef.h>
typedef struct { uintptr_t x[32], sepc, sstatus, scause, stval; } BiomiyaTrapFrame;
_Static_assert(sizeof(BiomiyaTrapFrame)==288,"TrapFrame size");
_Static_assert(offsetof(BiomiyaTrapFrame,sepc)==256,"TrapFrame offset");
void __biomiya_panic(const unsigned char *, uintptr_t) __attribute__((noreturn));
void __biomiya_dispatch(BiomiyaTrapFrame *,void (*)(BiomiyaTrapFrame *),uintptr_t) __attribute__((noreturn));
void __biomiya_trap_entry(BiomiyaTrapFrame *) __attribute__((noreturn));
void __biomiya_platform_fault(void) __attribute__((noreturn));
#endif
