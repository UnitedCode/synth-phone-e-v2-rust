// memory.rs
use alloc_cortex_m::CortexMHeap;

pub static ALLOCATOR: CortexMHeap = CortexMHeap::empty();

pub fn init_allocator(heap_start: usize, heap_size: usize) {
    unsafe { ALLOCATOR.init(heap_start, heap_size) }
}
