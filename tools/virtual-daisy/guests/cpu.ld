ENTRY(probe_main)
SECTIONS {
    .vectors 0x6000 : { KEEP(*(.vectors)) }
    .text 0xa000 : { *(.text*) *(.rodata*) }
    .data 0xe000 : { *(.data*) }
    .bss 0xf000 (NOLOAD) : { *(.bss*) *(COMMON) }
    .flashprobe 0x08010000 : { *(.flashprobe*) }
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) }
}
