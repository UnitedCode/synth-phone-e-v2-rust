//! Independent Thumb guest: real MMIO, WFI, NVIC delivery and persistent NOR data.
struct Thumb {
    code: Vec<u16>,
    literals: Vec<(usize, u16, u32)>,
}
impl Thumb {
    fn new() -> Self {
        Self {
            code: Vec::new(),
            literals: Vec::new(),
        }
    }
    fn load(&mut self, register: u16, value: u32) {
        self.literals.push((self.code.len(), register, value));
        self.code.push(0);
    }
    fn emit(&mut self, code: &[u16]) {
        self.code.extend_from_slice(code);
    }
    fn write(&mut self, offset: u16, value: u32) {
        self.load(0, value);
        self.emit(&[0x6030 | ((offset / 4) << 6)]); // STR r0,[r6,#offset].
    }
    fn read_result(&mut self, offset: u16, word: u16) {
        self.emit(&[0x6830 | ((offset / 4) << 6), 0x6028 | (word << 6)]);
    }
    fn finish(mut self) -> Vec<u16> {
        if !self.code.len().is_multiple_of(2) {
            self.code.push(0xbf00);
        }
        for (index, register, value) in self.literals {
            let offset = self.code.len() * 2 - ((index * 2 + 4) & !3);
            assert!(offset.is_multiple_of(4) && offset <= 1020);
            self.code[index] = 0x4800 | (register << 8) | (offset as u16 / 4);
            self.code.extend([value as u16, (value >> 16) as u16]);
        }
        self.code
    }
}
fn configure(t: &mut Thumb) {
    t.write(0, 1 | (3 << 24)); // Reset HSI64 /4 = 16MHz serial clock.
    t.write(4, 22 << 16); // Physical 8MiB.
}
fn read_array(t: &mut Thumb, word: u16) {
    t.write(0x10, 3);
    t.write(0x14, 0x05002503);
    t.write(0x18, 0x100);
    t.read_result(0x20, word);
}
fn wait_ready_irq(t: &mut Thumb) {
    t.write(0x10, 0);
    t.write(0x24, 1); // Match WIP=0, not WEL.
    t.write(0x28, 0);
    t.write(0x2c, 255); // Sparse hardware polling; no software spin loop.
    t.write(0xc, 0x1b);
    t.write(0, 1 | (3 << 24) | (1 << 22) | (1 << 19)); // APMS, SMIE.
    t.write(0x14, 0x09000105);
    t.emit(&[0xbf30]); // WFI; the actual peripheral raises IRQ92.
}
pub fn image(enable_irq: bool, corrupt_program: bool) -> Vec<u8> {
    let mut t = Thumb::new();
    t.load(5, 0x2001a000);
    t.load(6, 0x52005000);
    t.load(7, 0x580244d4);
    t.load(0, 1 << 14);
    t.emit(&[0x6038]); // RCC.AHB3ENR.QSPIEN, reset clock scenario.
    if enable_irq {
        t.load(7, 0xe000e108);
        t.load(0, 1 << 28);
        t.emit(&[0x6038]); // ISER2 IRQ92.
    }
    configure(&mut t);
    t.write(0x10, 2);
    t.write(0x14, 0x0500019f);
    t.read_result(0x20, 1);
    t.write(0x14, 0x106);
    t.write(0x10, 3);
    t.write(0x14, 0x01002502);
    t.write(0x18, 0x100);
    t.write(
        0x20,
        if corrupt_program {
            0x47702008
        } else {
            0x47702009
        },
    );
    t.write(0x10, 0);
    t.write(0x14, 0x05000105);
    t.read_result(0x20, 2); // WIP=1 and WEL=1 before NVM completion.
    wait_ready_irq(&mut t);
    read_array(&mut t, 3);
    t.load(7, 0x5802447c);
    t.load(0, 1 << 14);
    t.emit(&[0x6038, 0x2000, 0x6038]); // Reset just the STM32 controller.
    configure(&mut t);
    read_array(&mut t, 4); // Independent flash storage must survive.
    t.load(7, 0xe000ed14);
    t.load(0, 3 << 16);
    t.emit(&[0x6038]); // Enable distinct M7 data/instruction caches.
    t.write(0x14, 0x0d002503); // Mapped SPI normal read, same physical array.
    t.load(4, 0x90000100);
    t.load(7, 0x90000101);
    t.emit(&[0x6820, 0x6268, 0x47b8, 0x62a8]); // Data, then execute MOVS r0,#9; BX LR.
    t.write(0, 3 | (3 << 24)); // Abort mapped before programming.
    t.write(0x14, 0x106);
    t.write(0x10, 3);
    t.write(0x14, 0x01002502);
    t.write(0x18, 0x100);
    t.write(0x20, 0x47702001); // Only 1->0 transitions, no intervening erase.
    wait_ready_irq(&mut t);
    t.write(0x14, 0x0d002503);
    t.emit(&[0x6820, 0x62e8, 0x47b8, 0x6328]); // Both caches must remain stale.
    t.load(0, 0xe000ef5c);
    t.emit(&[0x6004, 0xf3bf, 0x8f4f, 0x6820, 0x6368]); // DCIMVAC; DSB; fresh data.
    t.load(0, 0xe000ef58);
    t.emit(&[0x6004, 0xf3bf, 0x8f4f, 0xf3bf, 0x8f6f, 0x47b8, 0x63a8]); // ICIMVAU; DSB; ISB; fresh execution.
    t.write(0, 3 | (3 << 24));
    t.load(7, 0x58024400);
    t.load(0, 0x81);
    t.emit(&[0x6038]); // Enable CSI alongside the HSI system clock.
    t.load(7, 0x5802444c);
    t.load(0, 0x10000030);
    t.emit(&[0x6038]); // QSPI kernel CKPER=CSI4MHz, serial=1MHz.
    t.write(0x30, 5000);
    t.write(0xc, 0x1b);
    t.write(0, 9 | (3 << 24) | (1 << 20)); // Timeout counter and TOIE.
    t.write(0x14, 0x0d002503);
    t.load(0, 0x90000120);
    // A short guest delay crosses the CPU's execution quantum so the managed
    // host schedules the slow deadline before the kernel clock changes.
    t.emit(&[0x6800, 0xf3bf, 0x8f4f, 0xf3bf, 0x8f6f]);
    t.load(1, 4096);
    t.emit(&[0x3901, 0xd1fd]);
    t.load(0, 0x30);
    t.emit(&[0x6038, 0xbf30]); // Switch CKPER to HSI64; scheduled IRQ must move earlier.
    t.write(0, 3 | (3 << 24));
    configure(&mut t);
    t.emit(&[0x2042, 0x6028]);
    t.emit(&[0x6a28, 0x2800, 0xd0fc]); // Host mailbox at +32 releases erase.
    t.write(0x14, 0x106);
    t.write(0x14, 0x25d7);
    t.write(0x18, 0x123); // Unaligned address selects the containing 4KiB sector.
    wait_ready_irq(&mut t);
    read_array(&mut t, 5);
    t.emit(&[0x20ab, 0x6028, 0xe7fe]);
    let mut code = t.finish();
    assert!(code.len() < 0x400);
    code.resize(0x400, 0xbf00); // Handler fixed at ITCM 0x9800.
    let mut handler = Thumb::new();
    handler.emit(&[0xb403]); // Preserve r0,r1.
    handler.load(0, 0x52005008);
    handler.emit(&[0x6801, 0x2018, 0x4001]); // AND SR with SMF|TOF.
    handler.load(0, 0x2001a03c);
    handler.emit(&[0x6800, 0x4301]); // Aggregate observed interrupt sources.
    handler.load(0, 0x2001a03c);
    handler.emit(&[0x6001]);
    handler.load(0, 0x5200500c);
    handler.emit(&[0x2118, 0x6001]); // Deassert SMF/TOF before return.
    handler.load(0, 0x2001a018);
    handler.emit(&[0x6801, 0x3101, 0x6001, 0xbc03, 0x4770]);
    code.extend(handler.finish());
    code.into_iter().flat_map(u16::to_le_bytes).collect()
}
pub fn check(prefix: &std::path::Path) -> super::Result<()> {
    super::grade::execution_evidence(prefix)?;
    let report = super::grade::artifact(prefix, ".storage.txt");
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    for (suffix, expected) in [
        (".storage-early.bin", vec![0]),
        (
            ".storage-program.bin",
            vec![0x42, 0x17609d, 3, 0x47702009, 0x47702009, 0, 3],
        ),
        (
            ".storage.bin",
            vec![0xab, 0x17609d, 3, 0x47702009, 0x47702009, u32::MAX, 4],
        ),
        (
            ".storage-cache.bin",
            vec![0x47702009, 9, 0x47702009, 9, 0x47702001, 1, 0x18],
        ),
    ] {
        let data = std::fs::read(super::grade::artifact(prefix, suffix))?;
        if data.len() != expected.len() * 4 {
            return Err("Truncated ARM storage capture".into());
        }
        for (i, (&bytes, expected)) in data.as_chunks::<4>().0.iter().zip(expected).enumerate() {
            let actual = u32::from_le_bytes(bytes);
            if actual != expected {
                return Err(format!(
                    "ARM storage {suffix} word {i}: expected {expected:#x}, got {actual:#x}"
                )
                .into());
            }
        }
    }
    std::fs::write(report, "arm_qspi_program_and_sector_erase=passed\ncontroller_reset_preserves_external_nor=passed\nautomatic_polling_irq92_wfi=passed\nprogram_pending_at_50us=passed\nirq_count=4\nmapped_data_and_execution=passed\nexternal_program_requires_explicit_dcache_and_icache_invalidation=passed\nmapped_timeout_irq_and_clock_rescheduling=passed\nserial_edges_and_physical_timing=UNVERIFIED\n")?;
    Ok(())
}
