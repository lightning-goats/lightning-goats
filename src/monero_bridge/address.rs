//! Strict Monero subaddress decoding. Keccak is used ONLY for the public four-byte
//! typo checksum, not signatures/authentication/key derivation. No curve ownership
//! is inferred; receiving authority still comes from the trusted provider.
const B58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

pub fn valid(s: &str, network: &str) -> bool {
    if s.len() != 95 || !s.is_ascii() {
        return false;
    }
    let mut decoded = Vec::with_capacity(69);
    for part in s.as_bytes().chunks(11) {
        let width = match part.len() {
            11 => 8,
            7 => 5,
            _ => return false,
        };
        let mut n = 0u64;
        for ch in part {
            let Some(d) = B58.iter().position(|b| b == ch) else {
                return false;
            };
            let Some(v) = n.checked_mul(58).and_then(|v| v.checked_add(d as u64)) else {
                return false;
            };
            n = v;
        }
        if width == 5 && n >= 1u64 << 40 {
            return false;
        }
        decoded.extend_from_slice(&n.to_be_bytes()[8 - width..]);
    }
    let tag = match network {
        "mainnet" => 42,
        "testnet" => 63,
        "stagenet" => 36,
        _ => return false,
    };
    decoded[0] == tag && checksum(&decoded[..65]) == decoded[65..]
}

fn checksum(data: &[u8]) -> [u8; 4] {
    // A subaddress is exactly one 136-byte Keccak-256 rate block, with legacy
    // Keccak domain padding 0x01 (NOT SHA3's 0x06).
    let mut block = [0u8; 136];
    block[..data.len()].copy_from_slice(data);
    block[data.len()] = 1;
    block[135] |= 128;
    let mut a = [0u64; 25];
    for (i, c) in block.chunks_exact(8).enumerate() {
        a[i] = u64::from_le_bytes(c.try_into().unwrap());
    }
    const ROT: [u32; 25] = [
        0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56,
        14,
    ];
    const RC: [u64; 24] = [
        1,
        0x8082,
        0x800000000000808a,
        0x8000000080008000,
        0x808b,
        0x80000001,
        0x8000000080008081,
        0x8000000000008009,
        0x8a,
        0x88,
        0x80008009,
        0x8000000a,
        0x8000808b,
        0x800000000000008b,
        0x8000000000008089,
        0x8000000000008003,
        0x8000000000008002,
        0x8000000000000080,
        0x800a,
        0x800000008000000a,
        0x8000000080008081,
        0x8000000000008080,
        0x80000001,
        0x8000000080008008,
    ];
    for rc in RC {
        let c: [u64; 5] =
            std::array::from_fn(|x| a[x] ^ a[x + 5] ^ a[x + 10] ^ a[x + 15] ^ a[x + 20]);
        for x in 0..5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                a[x + 5 * y] ^= d;
            }
        }
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                b[y + 5 * ((2 * x + 3 * y) % 5)] = a[x + 5 * y].rotate_left(ROT[x + 5 * y]);
            }
        }
        for x in 0..5 {
            for y in 0..5 {
                a[x + 5 * y] = b[x + 5 * y] ^ ((!b[(x + 1) % 5 + 5 * y]) & b[(x + 2) % 5 + 5 * y]);
            }
        }
        a[0] ^= rc;
    }
    a[0].to_le_bytes()[..4].try_into().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_public_subaddress_and_checksum_mutations() {
        let s = "84WsptnLmjTYQjm52SMkhQWsepprkcchNguxdyLkURTSW1WLo3tShTnCRvepijbc2X8GAKPGxJK9hfQhLHzoKSxh7y8Yqrg";
        assert!(valid(s, "mainnet"));
        assert!(!valid(s, "stagenet"));
        assert!(!valid(s, "testnet"));
        for i in 0..95 {
            let mut b = s.as_bytes().to_vec();
            b[i] = if b[i] == b'1' { b'2' } else { b'1' };
            assert!(!valid(std::str::from_utf8(&b).unwrap(), "mainnet"));
        }
        assert!(!valid(&"z".repeat(95), "mainnet"));
        assert_eq!(checksum(b""), [0xc5, 0xd2, 0x46, 0x01]);
        assert_eq!(checksum(b"abc"), [0x4e, 0x03, 0x65, 0x7a]);
    }
}
