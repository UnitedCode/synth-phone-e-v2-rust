/* MPS2 AN500 addresses, constrained to Daisy-sized flash and DTCM capacities.
 * This is not the Daisy memory map and does not model flash/cache wait states. */
MEMORY {
  FLASH : ORIGIN = 0x00000000, LENGTH = 128K
  RAM   : ORIGIN = 0x20000000, LENGTH = 128K
}
