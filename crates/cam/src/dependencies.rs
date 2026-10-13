//! Generation dependencies, distinct from the ordered simulation/NC program.
//!
//! A reordered operation needs a new generation only when its own inputs or
//! the earlier material-removal evidence it consumes changed. This is not a
//! collision certificate: setup assembly, target verification and rest-stock
//! replay must still use the new sequence.
use crate::{CamLinkingDto, CamOperationDto, CamRampType, CamSetupDto, DrillCycle};

pub const CAM_ORDER_DEPENDENCY_RULES_REVISION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CamOperationDependencyKind {
    IncomingStockHeight,
    PredrilledEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CamOperationDependency {
    pub operation_id: u64,
    pub kind: CamOperationDependencyKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlanningDependencyPolicy {
    pub incoming_stock_height: bool,
    pub predrilled_entry: bool,
    /// Reads the simulated stock every earlier operation leaves.
    pub remaining_stock: bool,
}

/// Keep exhaustive: adding a strategy must explicitly declare every earlier
/// operation fact its planner reads. Rest-from-setup evidence is separately
/// tracked by the host's transitive upstream-setup fingerprint.
/// Roughing starts from what earlier operations leave; any operation whose
/// feed height lies below the billet top may prove its rapid approach clear
/// against that simulated remaining stock.
pub(crate) fn consumes_remaining_stock(setup: &CamSetupDto, operation: &CamOperationDto) -> bool {
    matches!(
        operation,
        CamOperationDto::Adaptive3d { .. } | CamOperationDto::Flat3d { .. }
    ) || operation.feed_height_z() < setup.stock.max.z - 1e-9
}

pub(crate) fn planning_dependency_policy(
    setup: &CamSetupDto,
    operation: &CamOperationDto,
    linking: Option<&CamLinkingDto>,
) -> PlanningDependencyPolicy {
    let predrilled_entry = match operation {
        CamOperationDto::Contour2d { .. } => {
            linking.is_some_and(|l| !l.predrill_positions.is_empty())
        }

        CamOperationDto::Adaptive3d { .. } => {
            linking.is_some_and(|l| l.ramp_type == CamRampType::Predrill)
        }
        CamOperationDto::Face { .. }
        | CamOperationDto::Flat3d { .. }
        | CamOperationDto::Drill { .. }
        | CamOperationDto::Pocket2d { .. }
        | CamOperationDto::Chamfer2d { .. }
        | CamOperationDto::Thread { .. } => false,
    };
    PlanningDependencyPolicy {
        incoming_stock_height: true,
        predrilled_entry,
        remaining_stock: consumes_remaining_stock(setup, operation),
    }
}

pub fn cam_operation_dependencies(
    setup: &CamSetupDto,
    operation: &CamOperationDto,
    linking: Option<&CamLinkingDto>,
) -> Vec<CamOperationDependency> {
    let policy = planning_dependency_policy(setup, operation, linking);
    let mut dependencies = Vec::new();
    for source in setup
        .operations
        .iter()
        .take_while(|o| o.id() != operation.id())
        .filter(|o| o.enabled())
    {
        let kind = match source {
            _ if policy.remaining_stock => Some(CamOperationDependencyKind::IncomingStockHeight),
            CamOperationDto::Face { .. } if policy.incoming_stock_height => {
                Some(CamOperationDependencyKind::IncomingStockHeight)
            }
            CamOperationDto::Drill { cycle, .. }
                if policy.predrilled_entry
                    && matches!(
                        cycle,
                        DrillCycle::Drill | DrillCycle::ChipBreaking | DrillCycle::DeepHole
                    ) =>
            {
                Some(CamOperationDependencyKind::PredrilledEntry)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            dependencies.push(CamOperationDependency {
                operation_id: source.id(),
                kind,
            });
        }
    }

    dependencies.sort_by_key(|d| (d.kind, d.operation_id));
    dependencies
}
