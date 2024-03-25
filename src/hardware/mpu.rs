//!Configure MPU

//! This module provides functionalities to configure the Memory Protection Unit (MPU)
//! of ARM Cortex-M processors. The MPU is a critical component in embedded systems for
//! ensuring memory access control and protecting the integrity of the system. By defining
//! specific regions with tailored access permissions, the MPU can prevent erroneous or
//! malicious code from accessing unauthorized memory, thus enhancing the system's security
//! and stability.
//!
//! The provided functions enable and disable the MPU, configure memory regions for Direct
//! Memory Access (DMA) and for use with Static Dynamic Random-Access Memory (SDRAM). These
//! configurations ensure that memory accesses are performed safely and in accordance with
//! the specific requirements of the application, such as alignment and access rights.
//!
//! ## Key Features
//! - Enable and disable the MPU with safety checks.
//! - Configure memory regions for DMA, ensuring safe data transfer operations.
//! - Configure memory regions for SDRAM, optimizing access for performance and safety.
//! - Perform checks to ensure memory regions are correctly aligned and sized according to
//!   ARM®v7-M architecture requirements.
//!
//! ## Example Usage
//! This module is typically used in embedded systems where precise control over memory
//! access is required. For instance, when setting up a DMA operation, the `dma_init` function
//! can be used to configure a memory region that is safe for DMA transfers, ensuring that the
//! DMA controller does not access memory regions that could corrupt system state or user data.
//!
//! Similarly, the `sdram_init` function can be used when initializing SDRAM, to configure
//! the MPU for optimal access patterns and protection settings tailored to the needs of the
//! application and the characteristics of the SDRAM.
//!
//! ## Safety Considerations
//! The use of the MPU must be carefully considered and correctly implemented to ensure
//! that the memory protection contributes positively to system stability and security.
//! Incorrect configuration of the MPU can lead to system crashes, data corruption, or
//! security vulnerabilities.
//!
//! ## References
//! - ARM®v7-M Architecture Reference Manual (ARM DDI 0403)
//!
//! ## Example
//! ```no_run
//! // Example initialization of a DMA memory region
//! let mpu = cortex_m::peripheral::MPU::take().unwrap();
//! let scb = cortex_m::peripheral::SCB::take().unwrap();
//! let memory_region_start = 0x2001_0000 as *mut u32; // Example address
//! let region_size = 1024; // Example size
//!
//! dma_init(&mut mpu, &mut scb, memory_region_start, region_size);
//! ```
//!
//! **Note**: This module requires careful integration with the system's memory layout and
//! understanding of the ARM Cortex-M MPU functionality. It is intended for advanced users
//! familiar with embedded systems development and the specific hardware details of their
//! platform.


/// Based on example from:
/// https://github.com/richardeoin/stm32h7-fmc/blob/master/examples/stm32h747i-disco.rs
///
/// Memory address in location will be 32-byte aligned.
///
/// # Panics
///
/// Function will panic if `size` is not a power of 2. Function
/// will panic if `size` is not at least 32 bytes.

/// Refer to ARM®v7-M Architecture Reference Manual ARM DDI 0403
/// Version E.b Section B3.5
use log::info;

const MEMFAULTENA: u32 = 1 << 16;
const REGION_FULL_ACCESS: u32 = 0x03;
const REGION_ENABLE: u32 = 0x01;

pub fn disable(mpu: &mut cortex_m::peripheral::MPU, scb: &mut cortex_m::peripheral::SCB) {
    unsafe {
        /* Make sure outstanding transfers are done */
        cortex_m::asm::dmb();
        scb.shcsr.modify(|r| r & !MEMFAULTENA);
        /* Disable the MPU and clear the control register*/
        mpu.ctrl.write(0);
    }
}

pub fn enable(mpu: &mut cortex_m::peripheral::MPU, scb: &mut cortex_m::peripheral::SCB) {
    const MPU_ENABLE: u32 = 0x01;
    const MPU_DEFAULT_MMAP_FOR_PRIVILEGED: u32 = 0x04;

    unsafe {
        mpu.ctrl
            .modify(|r| r | MPU_DEFAULT_MMAP_FOR_PRIVILEGED | MPU_ENABLE);

        scb.shcsr.modify(|r| r | MEMFAULTENA);

        // Ensure MPU settings take effect
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
}

fn log2minus1(sz: u32) -> u32 {
    for x in 5..=31 {
        if sz == (1 << x) {
            return x - 1;
        }
    }
    panic!("Unknown memory region size!");
}

/// Setup MPU for dma
pub fn dma_init(
    mpu: &mut cortex_m::peripheral::MPU,
    scb: &mut cortex_m::peripheral::SCB,
    location: *mut u32,
    size: usize,
) {
    disable(mpu, scb);

    const REGION_NUMBER0: u32 = 0x00;
    const REGION_SHAREABLE: u32 = 0x01;
    const REGION_TEX: u32 = 0b001;
    const REGION_CB: u32 = 0b00;

    assert_eq!(
        size & (size - 1),
        0,
        "Memory region size must be a power of 2"
    );
    assert_eq!(
        size & 0x1F,
        0,
        "Memory region size must be 32 bytes or more"
    );

    info!("Memory Size 0x{:x}", log2minus1(size as u32));

    // Configure region 0
    //
    // Strongly ordered
    unsafe {
        mpu.rnr.write(REGION_NUMBER0);
        mpu.rbar.write((location as u32) & !0x1F);
        mpu.rasr.write(
            (REGION_FULL_ACCESS << 24)
                | (REGION_TEX << 19)
                | (REGION_SHAREABLE << 18)
                | (REGION_CB << 16)
                | (log2minus1(size as u32) << 1)
                | REGION_ENABLE,
        );
    }

    enable(mpu, scb);
}

/// Setup MPU for the sdram
pub fn sdram_init(
    mpu: &mut cortex_m::peripheral::MPU,
    scb: &mut cortex_m::peripheral::SCB,
    location: *mut u32,
    size: usize,
) {
    disable(mpu, scb);

    // SDRAM
    const REGION_NUMBER1: u32 = 0x01;

    assert_eq!(
        size & (size - 1),
        0,
        "SDRAM memory region size must be a power of 2"
    );
    assert_eq!(
        size & 0x1F,
        0,
        "SDRAM memory region size must be 32 bytes or more"
    );

    info!("SDRAM Memory Size 0x{:x}", log2minus1(size as u32));

    // Configure region 1
    //
    // Strongly ordered
    unsafe {
        mpu.rnr.write(REGION_NUMBER1);
        mpu.rbar.write((location as u32) & !0x1F);
        mpu.rasr
            .write((REGION_FULL_ACCESS << 24) | (log2minus1(size as u32) << 1) | REGION_ENABLE);
    }

    enable(mpu, scb);
}