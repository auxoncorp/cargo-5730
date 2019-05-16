#![cfg_attr(not(feature = "std"), no_std)]

pub fn add(x: u32, y: u32) -> u32 {
    #[cfg(feature="std")]
    {
        println!("Adding the numbers {} and {}", x, y);
    }

    x + y
}
