#include "runtime.h"
int __biomiya_test_entry(void) {volatile unsigned short *flash=(void*)0x20000000;__asm__ volatile("fence iorw,iorw":::"memory");flash[0]=0x98;unsigned short q=flash[0x20];unsigned short r=flash[0x22];unsigned short y=flash[0x24];flash[0]=0xff;__asm__ volatile("fence iorw,iorw":::"memory");return q=='Q'&&r=='R'&&y=='Y' ? 1 : -1;}
