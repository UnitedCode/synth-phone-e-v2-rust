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
    maintain(&mut code, &mut literals, 0xe000ef50);
    result(&mut code, 8); // ICIALLU exposes DMA's new instruction, returning 3.
    code.extend([0x2043, 0x6268, 0xe7fe]); // Final sentinel at results+36.
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
    let data = std::fs::read(super::grade::artifact(prefix, ".architecture.bin"))?;
    // Architectural outcomes, checked against processor-written public memory.
    let expected = [1, 0x47702002, 1, 1, 1, 2, 0x42, 2, 3, 0x43];
    if data.len() != 40 {
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
    std::fs::write(super::grade::artifact(prefix,".architecture.txt"),"arm_generated_code_coherence=passed\narm_data_and_instruction_views=distinct\ndirty_code_negative_control=caught\nuncleaned_code_negative_control=caught\ndma_code_negative_control=caught\n")?;
    Ok(())
}
