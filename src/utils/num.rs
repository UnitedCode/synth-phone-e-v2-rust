const FBIPMAX: f32 = 0.999985;
const FBIPMIN: f32 = -FBIPMAX;
const F32_TO_S24_SCALE: f32 = 8388608.0; // 2 ** 23
const S24_TO_F32_SCALE: f32 = 1.0 / F32_TO_S24_SCALE;
const S24_SIGN: i32 = 0x800000;

/**
# 24-bit Signed Integer Representation

This module defines a `S24` struct to represent 24-bit signed integers. 
The Rust standard library does not include a native 24-bit integer type, 
which is a requirement for certain applications such as audio processing, 
where 24-bit depth is common. This custom type allows for handling 24-bit 
data with the necessary operations to convert to and from other numeric 
types, specifically tailored for applications that interface with hardware 
or formats requiring 24-bit integers.

## Purpose

- **Audio Processing**: In digital audio, 24-bit samples are widely used due to their high resolution. 
  This struct enables accurate representation and manipulation of audio samples.
  
- **Compatibility**: Ensures compatibility with external systems or protocols that utilize 24-bit data, 
  facilitating the exchange of information without loss of precision or the need for awkward workarounds.

## Features

- Conversion from and to `i32`, `u32`, and `f32`, making it versatile for operations that involve 
  floating-point computation or interfacing with APIs that use standard Rust numeric types.

- Custom implementations ensure that conversions respect the bounds of 24-bit integers, 
  preventing overflow and underflow, and maintaining the integrity of the data.

## Usage Example

```rust
let sample_f32: f32 = 0.5; // A sample floating-point value
let sample_s24: S24 = sample_f32.into(); // Convert to S24
let back_to_f32: f32 = sample_s24.into(); // And back to f32
``
*/

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct S24(i32);

impl From<i32> for S24 {
    fn from(x: i32) -> S24 {
        S24(x)
    }
}

impl From<u32> for S24 {
    fn from(x: u32) -> S24 {
        S24(x as i32)
    }
}

impl From<S24> for i32 {
    fn from(x: S24) -> i32 {
        x.0
    }
}

impl From<S24> for u32 {
    fn from(x: S24) -> u32 {
        x.0 as u32
    }
}

impl From<f32> for S24 {
    fn from(x: f32) -> S24 {
        let x = if x <= FBIPMIN {
            FBIPMIN
        } else if x >= FBIPMAX {
            FBIPMAX
        } else {
            x
        };
        S24((x * F32_TO_S24_SCALE) as i32)
    }
}

impl From<S24> for f32 {
    fn from(x: S24) -> f32 {
        ((x.0 ^ S24_SIGN) - S24_SIGN) as f32 * S24_TO_F32_SCALE
    }
}
