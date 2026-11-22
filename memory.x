MEMORY
{
	FLASH (RX)    : ORIGIN = 0x08000000, LENGTH = 128K
	DTCMRAM (RWX) : ORIGIN = 0x20000000, LENGTH = 128K
	SRAM (RWX)    : ORIGIN = 0x24000000, LENGTH = 512K
	RAM_D2 (RWX)  : ORIGIN = 0x30000000, LENGTH = 288K
	RAM_D3 (RWX)  : ORIGIN = 0x38000000, LENGTH = 64K
	ITCMRAM (RWX) : ORIGIN = 0x00000000, LENGTH = 64K
	SDRAM (RWX)   : ORIGIN = 0xc0000000, LENGTH = 64M
	QSPIFLASH (RX): ORIGIN = 0x90000000, LENGTH = 8M
}

/* stm32h7xx-hal uses a PROVIDE that expects RAM symbol to exist
*/
REGION_ALIAS(RAM, DTCMRAM);

SECTIONS
{
	/* CPU-only phases; keep DMA buffers together in their aligned D2 region. */
	.vocal_state (NOLOAD) :
	{
		. = ALIGN(32);
		*(.vocal_state)
		. = ALIGN(32);
	} > SRAM

	/* RTIC's CPU-owned resources used most of DTCM, leaving only 56 KiB
	 * for the initialization frame and nested FFT/audio/UI interrupt stacks.
	 * Place their explicitly initialized, NOLOAD storage in AXI SRAM instead.
	 * DMA buffers remain in their dedicated D2 section below. */
	.rtic_state (NOLOAD) :
	{
		. = ALIGN(32);
		_srtic_state = .;
		*(.uninit .uninit.*)
		. = ALIGN(32);
		_ertic_state = .;
	} > SRAM

	.sram1_bss (NOLOAD) :
	{
		. = ALIGN(4);
		_ssram1_bss = .;

		PROVIDE(__sram1_bss_start__ = _sram1_bss);
		*(.sram1_bss)
		*(.sram1_bss*)
		. = ALIGN(4);
		_esram1_bss = .;

		PROVIDE(__sram1_bss_end__ = _esram1_bss);
	} > RAM_D2

	.sdram (NOLOAD) :
	{
		. = ALIGN(4);
		_ssdram_bss = .;

		PROVIDE(__sdram_start = _ssdram_bss);
		*(.sdram)
		*(.sdram*)
		. = ALIGN(4);
		_esdram = .;

		PROVIDE(__sdram_end = _esdram);
	} > SDRAM


}

/* cortex-m-rt defines __euninit after the remaining DTCM data. Keep a
 * meaningful lower bound on stack headroom as the firmware grows. */
ASSERT(ORIGIN(DTCMRAM) + LENGTH(DTCMRAM) - __euninit >= 96K,
       "Synthphone requires at least 96 KiB of DTCM stack headroom");
