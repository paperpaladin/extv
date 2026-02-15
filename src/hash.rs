#![allow(dead_code)]
// The three directory hash functions ext4 uses, each in a signed and an
// unsigned form. Which one is in use comes from the directory's own
// index root, and whether the signed or the unsigned form applies comes
// from the superblock: the C in the original takes a char, and whether
// char is signed is an architecture decision, so a filesystem written
// on one machine records which it meant.
//
// None of this is a hash anyone would choose today. It is reproduced
// because the on-disk tree is ordered by it, and debugfs prints the
// hash of every name, so every name in a hashed directory is one
// comparison against an implementation we did not write.

pub const LEGACY: u8 = 0;
pub const HALF_MD4: u8 = 1;
pub const TEA: u8 = 2;
pub const LEGACY_UNSIGNED: u8 = 3;
pub const HALF_MD4_UNSIGNED: u8 = 4;
pub const TEA_UNSIGNED: u8 = 5;

const DELTA: u32 = 0x9E37_79B9;
const K1: u32 = 0;
const K2: u32 = 0x5A82_7999;
const K3: u32 = 0x6ED9_EBA1;

fn f(x: u32, y: u32, z: u32) -> u32 {
    z ^ (x & (y ^ z))
}
fn g(x: u32, y: u32, z: u32) -> u32 {
    (x & y).wrapping_add((x ^ y) & z)
}
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

fn half_md4(buf: &mut [u32; 4], input: &[u32; 8]) {
    let (mut a, mut b, mut c, mut d) = (buf[0], buf[1], buf[2], buf[3]);
    macro_rules! round {
        ($fun:ident, $a:ident, $b:ident, $c:ident, $d:ident, $x:expr, $s:expr) => {
            $a = $a.wrapping_add($fun($b, $c, $d)).wrapping_add($x);
            $a = $a.rotate_left($s);
        };
    }
    round!(f, a, b, c, d, input[0].wrapping_add(K1), 3);
    round!(f, d, a, b, c, input[1].wrapping_add(K1), 7);
    round!(f, c, d, a, b, input[2].wrapping_add(K1), 11);
    round!(f, b, c, d, a, input[3].wrapping_add(K1), 19);
    round!(f, a, b, c, d, input[4].wrapping_add(K1), 3);
    round!(f, d, a, b, c, input[5].wrapping_add(K1), 7);
    round!(f, c, d, a, b, input[6].wrapping_add(K1), 11);
    round!(f, b, c, d, a, input[7].wrapping_add(K1), 19);

    round!(g, a, b, c, d, input[1].wrapping_add(K2), 3);
    round!(g, d, a, b, c, input[3].wrapping_add(K2), 5);
    round!(g, c, d, a, b, input[5].wrapping_add(K2), 9);
    round!(g, b, c, d, a, input[7].wrapping_add(K2), 13);
    round!(g, a, b, c, d, input[0].wrapping_add(K2), 3);
    round!(g, d, a, b, c, input[2].wrapping_add(K2), 5);
    round!(g, c, d, a, b, input[4].wrapping_add(K2), 9);
    round!(g, b, c, d, a, input[6].wrapping_add(K2), 13);

    round!(h, a, b, c, d, input[3].wrapping_add(K3), 3);
    round!(h, d, a, b, c, input[7].wrapping_add(K3), 9);
    round!(h, c, d, a, b, input[2].wrapping_add(K3), 11);
    round!(h, b, c, d, a, input[6].wrapping_add(K3), 15);
    round!(h, a, b, c, d, input[1].wrapping_add(K3), 3);
    round!(h, d, a, b, c, input[5].wrapping_add(K3), 9);
    round!(h, c, d, a, b, input[0].wrapping_add(K3), 11);
    round!(h, b, c, d, a, input[4].wrapping_add(K3), 15);

    buf[0] = buf[0].wrapping_add(a);
    buf[1] = buf[1].wrapping_add(b);
    buf[2] = buf[2].wrapping_add(c);
    buf[3] = buf[3].wrapping_add(d);
}

fn tea(buf: &mut [u32; 4], input: &[u32; 4]) {
    let mut sum: u32 = 0;
    let (mut b0, mut b1) = (buf[0], buf[1]);
    let (a, b, c, d) = (input[0], input[1], input[2], input[3]);
    for _ in 0..16 {
        sum = sum.wrapping_add(DELTA);
        b0 = b0.wrapping_add(
            ((b1 << 4).wrapping_add(a)) ^ b1.wrapping_add(sum) ^ ((b1 >> 5).wrapping_add(b)),
        );
        b1 = b1.wrapping_add(
            ((b0 << 4).wrapping_add(c)) ^ b0.wrapping_add(sum) ^ ((b0 >> 5).wrapping_add(d)),
        );
    }
    buf[0] = buf[0].wrapping_add(b0);
    buf[1] = buf[1].wrapping_add(b1);
}

fn byte(b: u8, unsigned: bool) -> u32 {
    if unsigned {
        b as u32
    } else {
        // The original reads through a signed char, so a byte above
        // 0x7f contributes a negative number. This is the whole of the
        // difference between the two families, and it only shows on a
        // name with a byte that high: the corpus has one for that
        // reason.
        (b as i8) as i32 as u32
    }
}

// Pack up to num words of the name, big-endian within each word, with
// the remainder padded by a word made of the length repeated in all
// four bytes.
fn str2hashbuf(msg: &[u8], out: &mut [u32], num: usize, unsigned: bool) {
    let len0 = msg.len();
    let mut pad = len0 as u32 | ((len0 as u32) << 8);
    pad |= pad << 16;
    let mut val = pad;
    let len = len0.min(num * 4);
    let mut written = 0usize;
    for i in 0..len {
        val = byte(msg[i], unsigned).wrapping_add(val << 8);
        if i % 4 == 3 {
            out[written] = val;
            written += 1;
            val = pad;
        }
    }
    let mut left = num - written;
    if left > 0 {
        out[written] = val;
        written += 1;
        left -= 1;
    }
    for k in 0..left {
        out[written + k] = pad;
    }
}

fn legacy(name: &[u8], unsigned: bool) -> u32 {
    let mut hash0: u32 = 0x12a3_fe2d;
    let mut hash1: u32 = 0x37ab_e8f9;
    for &b in name {
        let c = byte(b, unsigned);
        // The original is signed arithmetic on an int, and the
        // subtraction below is how it keeps the value positive.
        let mut hash = hash1.wrapping_add(hash0 ^ c.wrapping_mul(7152373));
        if hash & 0x8000_0000 != 0 {
            hash = hash.wrapping_sub(0x7fff_ffff);
        }
        hash1 = hash0;
        hash0 = hash;
    }
    hash0 << 1
}

// Returns (hash, minor hash), the pair debugfs prints for every name in
// a hashed directory.
pub fn dirhash(version: u8, name: &[u8], seed: &[u32; 4]) -> (u32, u32) {
    let mut buf: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    if seed.iter().any(|&s| s != 0) {
        buf = *seed;
    }
    let (mut hash, mut minor) = (0u32, 0u32);
    match version {
        LEGACY | LEGACY_UNSIGNED => {
            hash = legacy(name, version == LEGACY_UNSIGNED);
        }
        HALF_MD4 | HALF_MD4_UNSIGNED => {
            let unsigned = version == HALF_MD4_UNSIGNED;
            // The C loop runs while there is name left, so an empty
            // name is hashed zero times rather than once.
            let mut off = 0;
            while off < name.len() {
                let mut input = [0u32; 8];
                str2hashbuf(&name[off..], &mut input, 8, unsigned);
                half_md4(&mut buf, &input);
                off += 32;
            }
            minor = buf[2];
            hash = buf[1];
        }
        TEA | TEA_UNSIGNED => {
            let unsigned = version == TEA_UNSIGNED;
            let mut off = 0;
            while off < name.len() {
                let mut input = [0u32; 4];
                str2hashbuf(&name[off..], &mut input, 4, unsigned);
                tea(&mut buf, &input);
                off += 16;
            }
            hash = buf[0];
            minor = buf[1];
        }
        _ => {}
    }
    hash &= !1;
    // 0x7fffffff is the end-of-file marker in a tree walk, so a name
    // that hashes to it is moved down by one.
    if hash == (0x7fff_ffffu32 << 1) {
        hash = 0x7fff_fffeu32 << 1;
    }
    (hash, minor)
}
