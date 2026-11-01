use std::sync::{Arc, Mutex};
use virtual_daisy::{clock::Rate, memory::SystemMemory, qspi::Qspi};
fn mapped(size: u32) -> (SystemMemory, Arc<Mutex<Qspi>>) {
    let controller = Arc::new(Mutex::new(Qspi::default()));
    let mut memory = SystemMemory::default();
    memory.attach_qspi(Arc::clone(&controller));
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    {
        let mut qspi = controller.lock().unwrap();
        qspi.configure_clock(Some(Rate {
            numerator: 16_000_000,
            denominator: 1,
        }))
        .unwrap();
        qspi.write(0, 1, 4).unwrap();
        qspi.write(4, size << 16, 4).unwrap();
        qspi.write(0x14, 0x0d002503, 4).unwrap();
    }
    (memory, controller)
}
#[test]
fn a_demand_vector_bus_error_finishes_the_existing_frame_before_native_fault_delivery() {
    let (mut memory, _) = mapped(5);
    for word in 0..8 {
        let mut value = u64::from(word + 10);
        memory
            .cpu_access(u64::MAX - 1, 0x3001ffe0 + word * 4, 4, true, &mut value)
            .unwrap();
    }
    let mut completed = false;
    for _ in 0..32 {
        let wait = memory.exception_entry(0x90000040).unwrap();
        if wait == 0 {
            completed = true;
            break;
        }
        memory.take_cpu_debt();
        memory.advance_time(memory.now() + wait).unwrap();
    }
    assert!(completed);
    assert_eq!(memory.now(), 40);
    assert_eq!(memory.exception_stack_words, 8);
    for word in 0..8 {
        assert_eq!(
            memory.read_dma(0x3001ffe0 + word * 4, 4).unwrap(),
            word + 10
        );
    }
    let mut vector = 0;
    let error = memory
        .cpu_access(u64::MAX, 0x90000040, 4, false, &mut vector)
        .unwrap_err();
    assert!(virtual_daisy::qspi::mapped_bus_error(error));
    memory.finish_exception_entry().unwrap();
}
#[test]
fn stopped_clock_and_prefetch_only_errors_remain_unsupported_vector_service() {
    let (mut memory, controller) = mapped(5);
    controller.lock().unwrap().configure_clock(None).unwrap();
    assert!(memory.exception_entry(0x9000000c).is_err());
    let (mut memory, _) = mapped(3); // Demanded word fits 16B, whole cache line does not.
    memory.write_scb(0xd14, 1 << 17).unwrap();
    assert!(memory.exception_entry(0x9000000c).is_err());
}
