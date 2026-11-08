//! CPU-owned streaming state in AXI SRAM, leaving DTCM for DSP/IRQ stacks.
use app_lib::vocal_frame::VocalStreamState;
use core::{
    mem::MaybeUninit,
    sync::atomic::{AtomicBool, Ordering},
};

#[link_section = ".vocal_state"]
static mut STATE: MaybeUninit<VocalStreamState> = MaybeUninit::uninit();
static TAKEN: AtomicBool = AtomicBool::new(false);

// Keep constructor scratch out of RTIC's already-large initialization frame.
#[inline(never)]
pub fn take() -> Option<&'static mut VocalStreamState> {
    if TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: only the successful caller can reach this storage. Initialize all
    // bytes explicitly: the NOLOAD SRAM section is not zeroed by cortex-m-rt.
    // RTIC then owns this unique reference as an FFT-task local resource.
    unsafe {
        let ptr = core::ptr::addr_of_mut!(STATE).cast::<VocalStreamState>();
        ptr.write(VocalStreamState::new());
        Some(&mut *ptr)
    }
}
