// https://docs.rust-embedded.org/embedonomicon/dma.html
use log::info;

use crate::{disable, enable};
pub trait DmaConfigurable {
    fn configure(&self, mpu: &mut cortex_m::peripheral::MPU, scb: &mut cortex_m::peripheral::SCB);
}

pub struct DmaPlaybackConfig {
    pub location: *mut u32,
    pub size: usize,
}

pub struct DmaRecordConfig {
    pub location: *mut u32,
    pub size: usize,
}

impl DmaConfigurable for DmaPlaybackConfig {
    fn configure(&self, mpu: &mut cortex_m::peripheral::MPU, scb: &mut cortex_m::peripheral::SCB) {
        assert!(self.size.is_power_of_two(), "Size must be a power of 2");
        assert!(self.size >= 32, "Size must be at least 32 bytes");
    
        disable(mpu, scb); // Make sure you've got a disable function defined or imported
    
        const REGION_NUMBER: u32 = 0;
        const SHAREABLE: u32 = 0b01;
        const TEX: u32 = 0b001;
        const CB: u32 = 0b00;
        const FULL_ACCESS: u32 = 0b11;
        const ENABLE: u32 = 0b1;
    
        let region_size = self.size.trailing_zeros() - 1; // Calculate the size parameter for MPU
    
        info!("Configuring MPU for DMA: size 0x{:X}", region_size);
    
        unsafe {
            mpu.rnr.write(REGION_NUMBER);
            mpu.rbar.write(self.location as u32 & !0x1F);
            mpu.rasr.write(
                (FULL_ACCESS << 24)
                    | (TEX << 19)
                    | (SHAREABLE << 18)
                    | (CB << 16)
                    | (region_size << 1)
                    | ENABLE,
            );
        }
    
        enable(mpu, scb); // Ensure you've got an enable function
    }
}

impl DmaConfigurable for DmaRecordConfig {
    fn configure(&self, mpu: &mut cortex_m::peripheral::MPU, scb: &mut cortex_m::peripheral::SCB) {
        // Implement DMA configuration for recording
        // This might involve setting up the MPU region, enabling DMA, etc.
        // Similar to the `configure_dma_mpu_region` example but specific to recording
    }
}
