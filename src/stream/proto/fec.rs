//!
//! This module provides a fec-rs [ReconstructShard] impl, that doesn't use [Vec]
//!

use fec_rs::ReconstructShard;

/// A struct that holds shard data as a mutable reference to a byte slice,
/// and an optional length.
///
/// The length of the array must be big enough, else there could be a panic when reconstructing shards.
pub struct ArrayShard<'a> {
    /// If [None] this shard isn't currently present
    pub len: Option<usize>,
    pub array: &'a mut [u8],
}

impl<'a> ArrayShard<'a> {}

unsafe impl<'a> ReconstructShard for ArrayShard<'a> {
    fn len(&self) -> Option<usize> {
        self.len
    }

    fn get(&self) -> Option<&[u8]> {
        self.len.map(|_| self.array as &[u8])
    }

    fn get_mut(&mut self) -> Option<&mut [u8]> {
        self.len.map(|_| self.array as &mut [u8])
    }

    fn initialize(&mut self, len: usize) {
        self.len = Some(len);

        // This might panic, but we are okay with that
        self.array[0..len].fill(0);
    }
}
