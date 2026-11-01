use std::sync::{Arc, Mutex};
use virtual_daisy::{clock::Rate, memory::SystemMemory, qspi::Qspi};
fn mapped(q: &mut Qspi) {
    q.write(0, 1, 4).unwrap();
    q.write(4, 22 << 16, 4).unwrap();
    q.write(0x14, 0x0d002503, 4).unwrap();
}
fn program(q: &mut Qspi, data: u32) {
    q.write(0, 3, 4).unwrap(); // Abort mapped transaction before indirect program.
    q.write(0x14, 0x106, 4).unwrap();
    q.write(0x10, 3, 4).unwrap();
    q.write(0x14, 0x01002502, 4).unwrap();
    q.write(0x18, 0, 4).unwrap();
    q.write(0x20, data, 4).unwrap();
}
#[test]
fn mapped_nor_shares_physical_bytes_but_not_m7_cache_coherence() {
    let q = Arc::new(Mutex::new(Qspi::default()));
    let mut m = SystemMemory::default();
    m.attach_qspi(Arc::clone(&q));
    m.write_scb(0xd14, 3 << 16).unwrap();
    {
        let mut chip = q.lock().unwrap();
        chip.write(0, 1, 4).unwrap();
        chip.write(4, 22 << 16, 4).unwrap();
        program(&mut chip, 0x47702009);
        chip.advance_time(200_000).unwrap();
        mapped(&mut chip);
    }
    assert_eq!(m.read_cpu(0x90000000, 4).unwrap(), 0x47702009);
    assert_eq!(m.read_instruction(0x90000000, 4).unwrap(), 0x47702009);
    {
        let mut chip = q.lock().unwrap();
        program(&mut chip, 0x47702001);
        chip.advance_time(400_000).unwrap();
        mapped(&mut chip);
    }
    assert_eq!(m.read_dma(0x90000000, 4).unwrap(), 0x47702001);
    assert_eq!(m.read_cpu(0x90000000, 4).unwrap(), 0x47702009);
    assert_eq!(m.read_instruction(0x90000000, 4).unwrap(), 0x47702009);
    m.write_scb(0xf5c, 0x90000003).unwrap(); // DCIMVAC ignores low address bits.
    m.write_scb(0xf58, 0x90000001).unwrap();
    assert_eq!(m.read_cpu(0x90000000, 4).unwrap(), 0x47702001);
    assert_eq!(m.read_instruction(0x90000000, 4).unwrap(), 0x47702001);
    assert!(m.write_cpu(0x90000000, 4, 0).is_err());
    assert!(m.write_dma(0x90000000, 4, 0).is_err());
    q.lock().unwrap().write(0, 0, 4).unwrap();
    assert_eq!(m.read_cpu(0x90000000, 4).unwrap(), 0x47702001); // Cache hit needs no external transaction.
    assert!(m.read_cpu(0x90000020, 4).is_err());
}
#[test]
fn mapped_reads_obey_fsize_clock_gate_timeout_and_abort() {
    let mut q = Qspi::default();
    q.configure_clock(Some(Rate {
        numerator: 50_000_000,
        denominator: 1,
    }))
    .unwrap();
    mapped(&mut q);
    assert_eq!(q.read(8, 4).unwrap() & 32, 0);
    assert!(q.read_mapped(0x800000, 1).is_err());
    q.write(0x30, 10, 4).unwrap();
    q.write(0, 9 | (1 << 20), 4).unwrap(); // TCEN + TOIE.
    assert_eq!(q.mapped_service_ns(0, 4).unwrap(), 1300); // 8+24+32+CSHT1 at 50MHz.
    assert_eq!(q.read_mapped(0, 4).unwrap(), [0xff; 4]);
    assert_eq!(q.read(8, 4).unwrap() & 32, 32);
    assert_eq!(q.next_event(), Some(5960)); // 8+24+256+LPTR10 clocks.
    q.advance_time(5959).unwrap();
    assert!(!q.irq());
    q.advance_time(5960).unwrap();
    assert!(q.irq());
    assert_eq!(q.read(8, 4).unwrap() & 0x30, 0x10);
    q.write(0xc, 16, 4).unwrap();
    assert!(!q.irq());
    q.configure_clock(None).unwrap();
    assert!(q.read_mapped(0, 4).is_err());
    q.configure_clock(Some(Rate {
        numerator: 50_000_000,
        denominator: 1,
    }))
    .unwrap();
    q.read_mapped(0, 4).unwrap();
    q.write(0, 3, 4).unwrap();
    assert_eq!(q.next_event(), None);
    assert!(q.read_mapped(0, 4).is_err());
}

#[test]
fn normal_read_cannot_silently_run_above_the_chips_50mhz_limit() {
    let mut q = Qspi::default();
    q.configure_clock(Some(Rate {
        numerator: 100_000_000,
        denominator: 1,
    }))
    .unwrap();
    q.write(0, 1, 4).unwrap();
    q.write(4, 22 << 16, 4).unwrap();
    assert!(q.write(0x14, 0x0d002503, 4).is_err());
    q.write(0, 1 | (1 << 24), 4).unwrap(); // /2 puts normal read in its rated range.
    q.write(0x14, 0x0d002503, 4).unwrap();
    assert_eq!(q.read_mapped(0, 4).unwrap(), [0xff; 4]);
}

#[test]
fn translation_peeks_do_not_start_a_serial_read_or_allocate_an_instruction_line() {
    let q = Arc::new(Mutex::new(Qspi::default()));
    let mut memory = SystemMemory::default();
    memory.attach_qspi(Arc::clone(&q));
    memory.write_scb(0xd14, 3 << 16).unwrap();
    assert_eq!(memory.decode_instruction(0x90000000, 4).unwrap(), u32::MAX);
    assert_eq!(q.lock().unwrap().read(8, 4).unwrap() & 32, 0);
    assert!(memory.stats().contains("instruction_cache_fills=0\n"));
    assert!(memory.read_instruction(0x90000000, 4).is_err()); // Runtime must validate mapped access.
}

#[test]
fn mapped_cpu_and_dma_reads_wait_on_the_shared_qspi_target_while_sram_progresses() {
    let q = Arc::new(Mutex::new(Qspi::default()));
    let rate = Rate {
        numerator: 50_000_000,
        denominator: 1,
    };
    {
        let mut chip = q.lock().unwrap();
        chip.configure_clock(Some(rate)).unwrap();
        mapped(&mut chip);
    }
    let mut memory = SystemMemory::default();
    memory.attach_qspi(q);
    memory.configure_bus(rate, rate, None);
    memory.inject_bus_load_kind(2, 10_000, false).unwrap();
    memory.advance_time(0).unwrap();
    assert!(memory.access_wait(0x90000000, 4, false, false).unwrap() > 0);
    let dma = memory.dma_submit(0x90000004, false).unwrap().unwrap();
    assert!(memory.access_wait(0x24000000, 2, false, true).unwrap() > 0);
    memory.advance_time(100).unwrap();
    assert_eq!(memory.access_wait(0x24000000, 2, false, true).unwrap(), 0);
    assert!(!memory.dma_ready(dma));
    memory.advance_time(10_000).unwrap();
    assert!(!memory.dma_ready(dma));
    memory.advance_time(12_600).unwrap();
    assert!(memory.dma_ready(dma));
    assert_eq!(memory.access_wait(0x90000000, 4, false, false).unwrap(), 0);
    assert_eq!(memory.read_cpu(0x90000000, 4).unwrap(), u32::MAX);
}

#[test]
fn a_mapped_timeout_retains_unelapsed_serial_clocks_while_the_kernel_is_gated() {
    let mut q = Qspi::default();
    let rate = Rate {
        numerator: 50_000_000,
        denominator: 1,
    };
    q.configure_clock(Some(rate)).unwrap();
    mapped(&mut q);
    q.write(0x30, 10, 4).unwrap();
    q.write(0, 9, 4).unwrap();
    q.read_mapped(0, 4).unwrap();
    assert_eq!(q.next_event(), Some(5960));
    q.advance_time(1000).unwrap();
    q.configure_clock(None).unwrap();
    q.advance_time(1_000_000).unwrap();
    assert_eq!(q.next_event(), None);
    q.configure_clock(Some(rate)).unwrap();
    assert_eq!(q.next_event(), Some(1_004_960));
    q.advance_time(1_004_959).unwrap();
    assert_eq!(q.read(8, 4).unwrap() & 16, 0);
    q.advance_time(1_004_960).unwrap();
    assert_eq!(q.read(8, 4).unwrap() & 16, 16);
}
