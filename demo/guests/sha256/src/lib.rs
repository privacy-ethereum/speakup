//! A minimal SHA-256 guest for the zk-vm benchmark.
//!
//! Memory is handed out through [`cabi_realloc`], the WebAssembly Component
//! Model's canonical reallocation export: the host calls
//! `cabi_realloc(0, 0, align, len + 32)` to allocate the message buffer plus
//! digest space, writes the message there, then calls [`hash`] with that
//! pointer and the message length. The guest pads the message and feeds each
//! 64-byte block to the host's SHA-256 compression precompile
//! ([`mpz_vm_sys::sha256_compress`]) — the VM proves the compression directly
//! rather than replaying its gates. It then writes the 32-byte digest just past
//! the message, reveals it through the VCI so the verifier learns it, and
//! returns the digest's address.

use std::alloc::Layout;

use mpz_vm_sys::{reveal, sha256_compress};

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// The Component Model canonical `realloc`:
/// `cabi_realloc(old_ptr, old_size, align, new_size) -> ptr`.
///
/// Delegates to the default global (system) allocator: a fresh block
/// (`old_ptr == 0`) is an [`std::alloc::alloc`], otherwise a
/// [`std::alloc::realloc`] of the existing block.
#[no_mangle]
pub extern "C" fn cabi_realloc(old_ptr: i32, old_size: i32, align: i32, new_size: i32) -> i32 {
    let align = (align as usize).max(1);
    let new_size = new_size as usize;
    unsafe {
        if old_ptr == 0 {
            let layout = Layout::from_size_align_unchecked(new_size, align);
            std::alloc::alloc(layout) as i32
        } else {
            let old_layout = Layout::from_size_align_unchecked(old_size as usize, align);
            std::alloc::realloc(old_ptr as *mut u8, old_layout, new_size) as i32
        }
    }
}

/// Hashes the `len` bytes at `ptr`, writes the digest just past the message
/// (the buffer must have been allocated with 32 spare bytes), reveals it,
/// and returns the digest's address.
#[no_mangle]
pub extern "C" fn hash(ptr: i32, len: i32) -> i32 {
    let digest_ptr = ptr + len;
    let msg = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let out = unsafe { std::slice::from_raw_parts_mut(digest_ptr as *mut u8, 32) };
    out.copy_from_slice(&sha256(msg));
    reveal(&*out).wait();
    digest_ptr
}

fn sha256(msg: &[u8]) -> [u8; 32] {
    let len = msg.len();
    let mut h = H0;
    let bitlen = (len as u64).wrapping_mul(8);
    // Padded length: message + 0x80 + zeros + 8-byte length, rounded to 64.
    let mut total = len + 9;
    if total % 64 != 0 {
        total += 64 - (total % 64);
    }

    let mut block = [0u8; 64];
    let mut pos = 0;
    while pos < total {
        for (i, b) in block.iter_mut().enumerate() {
            let idx = pos + i;
            *b = if idx < len {
                msg[idx]
            } else if idx == len {
                0x80
            } else if idx + 8 >= total {
                let shift = (total - 1 - idx) * 8;
                (bitlen >> shift) as u8
            } else {
                0
            };
        }
        sha256_compress(&mut h, &block);
        pos += 64;
    }

    let mut out = [0u8; 32];
    for (word, chunk) in h.iter().zip(out.chunks_exact_mut(4)) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    out
}
