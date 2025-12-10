#![allow(dead_code)]
// The two checksums ext4 uses. Neither is a general-purpose CRC call:
// e2fsprogs seeds them in a particular way and does not invert the
// result at the end, so a textbook implementation produces the right
// polynomial and the wrong number.

// Castagnoli, reflected, as ext2fs_crc32c_le. The table is built at
// compile time.
const fn crc32c_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0x82F6_3B78 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static CRC32C: [u32; 256] = crc32c_table();

pub fn crc32c(seed: u32, data: &[u8]) -> u32 {
    let mut c = seed;
    for &b in data {
        c = CRC32C[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c
}

// ext2fs_crc16: the reflected 0x8005 polynomial, used by the older
// uninit_bg group checksum and by nothing else here.
pub fn crc16(seed: u16, data: &[u8]) -> u16 {
    let mut c = seed;
    for &b in data {
        c ^= b as u16;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xA001 } else { c >> 1 };
        }
    }
    c
}
