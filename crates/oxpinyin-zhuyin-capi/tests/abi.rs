//! Black-box tests over the exported libzhuyin C interface.

#![expect(
    unsafe_code,
    reason = "calls C symbols through their header declarations"
)]

#[path = "abi/offsets.rs"]
mod offsets;
