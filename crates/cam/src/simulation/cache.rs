//! Ready-to-present results, separate from voxel checkpoints. A repeat click
//! does no planning, cutting, surface extraction, or target comparison.
use super::*;

const MAX_ENTRIES: usize = 8;
const MAX_BYTES: usize = 64 * 1024 * 1024;

struct Entry {
    key: Vec<u8>,
    result: CamSimulationResultDto,
    bytes: usize,
}

#[derive(Default)]
struct ResultCache {
    entries: VecDeque<Entry>,
    bytes: usize,
}

fn cache() -> &'static Mutex<ResultCache> {
    static CACHE: OnceLock<Mutex<ResultCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ResultCache::default()))
}

pub(super) fn key(
    document: &CamDocumentDto,
    request: &CamSimulationRequestDto,
) -> Result<Vec<u8>, CamPlanError> {
    let mut normalized = request.clone();
    if let Some(target) = &mut normalized.target {
        if target
            .cache_key
            .as_ref()
            .is_some_and(|key| !key.trim().is_empty())
        {
            target.meshes.clear();
        }
    }

    serde_json::to_vec(&(crate::machine::motion_document(document), normalized))
        .map_err(|error| CamPlanError(error.to_string()))
}

pub(super) fn get(key: &[u8]) -> Option<CamSimulationResultDto> {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let index = cache.entries.iter().position(|entry| entry.key == key)?;
    let entry = cache.entries.remove(index)?;
    let result = entry.result.clone();
    cache.entries.push_back(entry);
    Some(result)
}

pub(super) fn insert(key: Vec<u8>, result: &CamSimulationResultDto) {
    if result.completed_steps.is_some() && result.completed_steps != Some(0) {
        return;
    }
    let mesh_bytes = |mesh: &CamSimulationMeshDto| (mesh.positions.len() + mesh.normals.len()) * 4;
    let bytes = key.len()
        + std::mem::size_of::<Entry>()
        + result.stock_mesh.as_ref().map_or(0, mesh_bytes)
        + result.comparison.as_ref().map_or(0, |comparison| {
            comparison.excess_mesh.as_ref().map_or(0, mesh_bytes)
                + comparison.gouge_mesh.as_ref().map_or(0, mesh_bytes)
        })
        + result.steps.len() * std::mem::size_of::<CamSimulationStepDto>()
        + result.collisions.len() * std::mem::size_of::<CamSimulationCollisionDto>()
        + result
            .collisions
            .iter()
            .map(|collision| collision.message.len())
            .sum::<usize>()
        + result
            .warnings
            .iter()
            .map(|warning| warning.len())
            .sum::<usize>();
    if bytes > MAX_BYTES {
        return;
    }
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(index) = cache.entries.iter().position(|entry| entry.key == key) {
        cache.bytes -= cache.entries.remove(index).expect("located entry").bytes;
    }
    while cache.entries.len() >= MAX_ENTRIES || cache.bytes + bytes > MAX_BYTES {
        if let Some(entry) = cache.entries.pop_front() {
            cache.bytes -= entry.bytes;
        } else {
            break;
        }
    }
    cache.bytes += bytes;
    cache.entries.push_back(Entry {
        key,
        result: result.clone(),
        bytes,
    });
}
