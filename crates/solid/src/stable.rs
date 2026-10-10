use limo_cad_core::{BodyId, EdgeId, FaceId};

/// FNV-1a is deliberately fixed instead of `DefaultHasher`: IDs must match
/// across runs, hosts, Rust versions, and serialized document reloads.
fn stable_hash(domain: &[u8], body: BodyId, key: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in domain
        .iter()
        .copied()
        .chain(body.0.to_le_bytes())
        .chain(key.as_bytes().iter().copied())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    hash &= (1u64 << 53) - 1;

    if hash == 0 {
        1
    } else {
        hash
    }
}

pub fn face_id(body: BodyId, key: &str) -> FaceId {
    FaceId(stable_hash(b"face", body, key))
}

pub fn edge_id(body: BodyId, key: &str) -> EdgeId {
    EdgeId(stable_hash(b"edge", body, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_deterministic_namespaced_and_nonzero() {
        let body = BodyId(7);
        assert_eq!(face_id(body, "face:0"), face_id(body, "face:0"));
        assert_ne!(face_id(body, "face:0").0, edge_id(body, "face:0").0);
        assert_ne!(face_id(body, "face:0").0, 0);
        assert_ne!(face_id(body, "face:0"), face_id(body, "face:1"));
    }
}
