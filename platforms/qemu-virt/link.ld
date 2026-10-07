OUTPUT_ARCH(riscv)
ENTRY(_start)
PHDRS { text PT_LOAD FLAGS(5); rodata PT_LOAD FLAGS(4); data PT_LOAD FLAGS(6); }
SECTIONS {
 . = 0x80000000;
 __text_start = .;
 .text : { KEEP(*(.text.start)) *(.text .text.*) } :text
 __text_end = .;
 . = ALIGN(4096); .rodata : { *(.rodata .rodata.*) } :rodata
 . = ALIGN(4096); .data : { *(.data .data.*) *(.sdata .sdata.*) } :data
 __global_pointer$ = . + 0x800;
 . = ALIGN(4096); __bss_start = .;
 .bss (NOLOAD) : { *(.bss .bss.*) *(.sbss .sbss.*) *(COMMON) } :data
 . = ALIGN(16); __bss_end = .;
 .stacks (NOLOAD) : { __hart_states = .; . += 4 * 65536; __boot_stacks = .; . += 4 * 131072; } :data
 __image_end = .;
 /DISCARD/ : { *(.eh_frame*) *(.comment) }
}
