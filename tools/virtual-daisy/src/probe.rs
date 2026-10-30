//! Small Thumb program run by the actual emulated M7, independently of the ELF.
//! Generates code, records return values, and performs documented cache maintenance.
pub fn image() -> Vec<u8> {
    let mut code = Vec::<u16>::new();
    let mut literals = Vec::<(usize, u16, u32)>::new();
    fn load(code: &mut Vec<u16>, literals: &mut Vec<(usize, u16, u32)>, reg: u16, value: u32) {
        literals.push((code.len(), reg, value));
        code.push(0);
    }
    fn result(code: &mut Vec<u16>, word: u16) {
        code.extend([0x47a0, 0x6028 | (word << 6)]);
    } // blx r4; str r0,[r5,#word*4].
    fn maintain(code: &mut Vec<u16>, literals: &mut Vec<(usize, u16, u32)>, address: u32) {
        load(code, literals, 6, address);
        code.extend([0x6034, 0xf3bf, 0x8f4f, 0xf3bf, 0x8f6f]); // str r4,[r6]; dsb; isb.
    }
    load(&mut code, &mut literals, 4, 0x24040001); // BLX target with Thumb bit.
    load(&mut code, &mut literals, 5, 0x2001e000);
    result(&mut code, 0);
    load(&mut code, &mut literals, 1, 0x47702002);
    // Align r4 for data access while preserving the BLX target in r7.
    code.extend([0x4627, 0x3c01, 0x6021, 0x6823, 0x606b, 0x3401]);
    result(&mut code, 2);
    // Maintenance ignores low address bits; r4 is the Thumb pointer.
    maintain(&mut code, &mut literals, 0xe000ef58);
    result(&mut code, 3);
    maintain(&mut code, &mut literals, 0xe000ef64);
    result(&mut code, 4);
    maintain(&mut code, &mut literals, 0xe000ef58);
    result(&mut code, 5);
    code.extend([0x2042, 0x61a8]); // Phase-one sentinel at results+24.
    code.extend([0x6be8, 0x2800, 0xd0fc]); // Poll DTCM mailbox at results+60.
    result(&mut code, 7); // DMA changed RAM: I-cache must still return 2.
                          // Two other lines in the same two-way set, one flash and one RAM.
                          // This exercises the declared deterministic replacement policy, not a claim
                          // that silicon chooses the same victim sequence.
    load(&mut code, &mut literals, 4, 0x0801e001);
    result(&mut code, 8);
    load(&mut code, &mut literals, 4, 0x24042001);
    result(&mut code, 9);
    load(&mut code, &mut literals, 4, 0x24040001);
    result(&mut code, 10); // The old translated block must refetch and return 3.
    maintain(&mut code, &mut literals, 0xe000ef50);
    result(&mut code, 11);
    load(&mut code, &mut literals, 6, 0x51043100);
    code.extend([0x200f, 0x6030, 0x6830, 0x6368]); // CPU read QoS -> results+52.
    load(&mut code, &mut literals, 6, 0x51043108);
    code.extend([0x2001, 0x6030, 0x6830, 0x63a8]); // Read issue override -> results+56.
    code.extend([0x2043, 0x6328, 0xe7fe]); // Final sentinel at results+48.
    if !code.len().is_multiple_of(2) {
        code.push(0xbf00);
    }
    for (index, reg, value) in literals {
        let offset = (code.len() * 2) - ((index * 2 + 4) & !3);
        assert!(offset.is_multiple_of(4) && offset <= 1020);
        code[index] = 0x4800 | (reg << 8) | (offset as u16 / 4);
        code.extend([value as u16, (value >> 16) as u16]);
    }
    code.into_iter().flat_map(u16::to_le_bytes).collect()
}

pub fn check(prefix: &std::path::Path) -> super::Result<()> {
    let report = super::grade::artifact(prefix, ".architecture.txt");
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let data = std::fs::read(super::grade::artifact(prefix, ".architecture.bin"))?;
    // Architectural outcomes, checked against processor-written public memory.
    let expected = [1, 0x47702002, 1, 1, 1, 2, 0x42, 2, 9, 8, 3, 3, 0x43, 15, 1];
    if data.len() != 60 {
        return Err("Truncated ARM probe results".into());
    }
    for (i, (bytes, expected)) in data.as_chunks::<4>().0.iter().zip(expected).enumerate() {
        let actual = u32::from_le_bytes(*bytes);
        if actual != expected {
            return Err(format!(
                "ARM coherence result {i}: expected {expected:#x}, got {actual:#x}"
            )
            .into());
        }
    }
    std::fs::write(super::grade::artifact(prefix,".architecture.txt"),"arm_generated_code_coherence=passed\narm_data_and_instruction_views=distinct\ndirty_code_negative_control=caught\nuncleaned_code_negative_control=caught\ndma_code_negative_control=caught\nflash_execution=passed\nmixed_flash_ram_conflict_eviction=passed\nreused_translation_refetch=passed\neviction_victim_sequence=EMULATOR_POLICY_ONLY\n")?;
    Ok(())
}

/// Runs from ITCM while the only user-flash sector is erased/programmed.
pub fn flash_image() -> Vec<u8> {
    let mut code = Vec::<u16>::new();
    let mut literals = Vec::<(usize, u16, u32)>::new();
    fn load(code: &mut Vec<u16>, literals: &mut Vec<(usize, u16, u32)>, reg: u16, value: u32) {
        literals.push((code.len(), reg, value));
        code.push(0);
    }
    fn record(code: &mut Vec<u16>, word: u16) {
        code.push(0x6028 | (word << 6));
    }
    load(&mut code, &mut literals, 5, 0x2001d000);
    load(&mut code, &mut literals, 4, 0x0801e001);
    code.push(0x47a0);
    record(&mut code, 0);
    load(&mut code, &mut literals, 6, 0x52002000);
    load(&mut code, &mut literals, 1, 0x45670123);
    code.push(0x6071);
    load(&mut code, &mut literals, 1, 0xcdef89ab);
    code.push(0x6071);
    code.extend([0x2111, 0x6031]); // ACR: HSI64MHz/VOS3 scenario, LATENCY1/WRHIGHFREQ1.
    code.extend([0x21b4, 0x60f1, 0x6930]);
    record(&mut code, 1); // SER0, x64, START; SR.
    load(&mut code, &mut literals, 7, 0x0801e000);
    code.push(0x6838);
    record(&mut code, 2); // Must stall until erase completes.
    code.push(0x6930);
    record(&mut code, 3);
    load(&mut code, &mut literals, 1, 1 << 16);
    code.push(0x6171); // CCR EOP W1C.
    code.extend([0x2132, 0x60f1]);
    load(&mut code, &mut literals, 1, 0x4770202a);
    load(&mut code, &mut literals, 2, u32::MAX);
    code.extend([0xe9c7, 0x1200, 0x3708]); // STRD r1,r2,[r7]; add r7,8.
    for reg in [0, 1, 3] {
        load(&mut code, &mut literals, reg, u32::MAX);
    }
    code.extend([0xc70f, 0xe9c7, 0x1200]); // STMIA r7!,{r0-r3}; STRD last8 bytes.
    code.push(0x6930);
    record(&mut code, 4);
    load(&mut code, &mut literals, 7, 0x0801e000);
    code.push(0x6838);
    record(&mut code, 5);
    code.push(0x47a0);
    record(&mut code, 6); // Stale I-cache still returns9.
    load(&mut code, &mut literals, 2, 0xe000ef58);
    code.extend([0x6014, 0xf3bf, 0x8f4f, 0xf3bf, 0x8f6f, 0x47a0]);
    record(&mut code, 7);
    code.push(0x6930);
    record(&mut code, 8);
    code.push(0x2055);
    record(&mut code, 9);
    code.push(0xe7fe);
    if !code.len().is_multiple_of(2) {
        code.push(0xbf00);
    }
    for (index, reg, value) in literals {
        let offset = code.len() * 2 - ((index * 2 + 4) & !3);
        assert!(offset.is_multiple_of(4) && offset <= 1020);
        code[index] = 0x4800 | (reg << 8) | (offset as u16 / 4);
        code.extend([value as u16, (value >> 16) as u16]);
    }
    code.into_iter().flat_map(u16::to_le_bytes).collect()
}

pub fn check_flash(prefix: &std::path::Path) -> super::Result<()> {
    let grade = super::grade::artifact(prefix, ".flash-probe.txt");
    if grade.exists() {
        std::fs::remove_file(&grade)?;
    }
    let data = std::fs::read(super::grade::artifact(prefix, ".flash.bin"))?;
    let expected = [9, 5, u32::MAX, 1 << 16, 5, 0x4770202a, 9, 42, 1 << 16, 0x55];
    if data.len() != 40 {
        return Err("Truncated ARM flash probe".into());
    }
    for (i, (bytes, expected)) in data.as_chunks::<4>().0.iter().zip(expected).enumerate() {
        let actual = u32::from_le_bytes(*bytes);
        if actual != expected {
            return Err(
                format!("ARM flash result {i}: expected {expected:#x}, got {actual:#x}").into(),
            );
        }
    }
    std::fs::write(grade,"arm_flash_erase_and_program=passed\nflash_reads_wait_for_completion=passed\nprogrammed_instruction_cache_stays_stale_until_invalidation=passed\n")?;
    Ok(())
}
