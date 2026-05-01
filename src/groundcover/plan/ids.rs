use std::collections::HashSet;

use super::MasterSpec;

const GENERATED_STATIC_ID_PREFIX: &str = "gm_";
const MAX_GENERATED_STATIC_ID_LEN: usize = 19;

#[must_use]
fn generated_static_id(hash: u64) -> String {
    format!("{GENERATED_STATIC_ID_PREFIX}{hash:016x}")
}

pub(super) fn allocate_generated_static_id(
    source_id: &str,
    source_master: &MasterSpec,
    source_static_ids: &HashSet<String>,
    generated_static_ids: &mut HashSet<String>,
) -> String {
    for attempt in 0..u64::MAX {
        let candidate =
            generated_static_id(generated_static_hash(source_id, source_master, attempt));
        debug_assert!(candidate.len() <= MAX_GENERATED_STATIC_ID_LEN);
        if !source_static_ids.contains(&candidate) && generated_static_ids.insert(candidate.clone())
        {
            return candidate;
        }
    }

    unreachable!("exhausted generated static id collision attempts")
}

fn generated_static_hash(source_id: &str, source_master: &MasterSpec, attempt: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    hash = fnv1a(hash, source_master.name.as_bytes());
    hash = fnv1a(hash, &source_master.size.to_le_bytes());
    hash = fnv1a(hash, source_id.as_bytes());
    fnv1a(hash, &attempt.to_le_bytes())
}

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn master(name: &str, size: u64) -> MasterSpec {
        MasterSpec {
            name: name.to_owned(),
            size,
        }
    }

    #[test]
    fn generated_static_id_allocation_avoids_source_id_collisions() {
        let source_master = master("Source.esp", 42);
        let colliding_id =
            generated_static_id(generated_static_hash("flora_grass_01", &source_master, 0));
        let source_static_ids = HashSet::from([colliding_id.clone()]);
        let mut generated_static_ids = HashSet::new();

        let generated_id = allocate_generated_static_id(
            "flora_grass_01",
            &source_master,
            &source_static_ids,
            &mut generated_static_ids,
        );

        assert_ne!(generated_id, colliding_id);
        assert!(generated_id.len() <= MAX_GENERATED_STATIC_ID_LEN);
        assert!(generated_static_ids.contains(&generated_id));
    }
}
