ENTRY(_start)

PHDRS
{
    load    PT_LOAD    FLAGS(7);
    dynamic PT_DYNAMIC FLAGS(6);
}

SECTIONS
{
    . = 0;

    .text     : { *(.text._start) *(.text .text.*) } :load
    .rodata   : { *(.rodata .rodata.*) } :load
    .dynsym   : { *(.dynsym) } :load
    .dynstr   : { *(.dynstr) } :load
    .hash     : { *(.hash) } :load
    .rel.dyn  : { *(.rel.dyn) *(.rel.text*) *(.rel.rodata*) *(.rel.data*) } :load
    .dynamic  : { *(.dynamic) } :load :dynamic
    .data.rel.ro : { *(.data.rel.ro*) } :load
    .got      : { *(.got) *(.got.plt) } :load
    .data     : { *(.data .data.*) } :load
    .bss      : { *(.bss .bss.*) *(COMMON) } :load

    /DISCARD/ : {
        *(.eh_frame) *(.comment) *(.note*)
        *(.interp) *(.gnu.version*)
    }
}
