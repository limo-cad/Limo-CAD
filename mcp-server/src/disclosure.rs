use std::collections::HashMap;

use serde_json::{json, Value};

pub const FOCUS_THROTTLE_MS: u64 = 300;
pub const SOFT_TTL_MS: u64 = 60_000;
pub const SOFT_REPROMOTE_MS: u64 = 15_000;
pub const SOFT_LRU: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusPack {
    Document,
    Assembly,
    Sketch,
    Solid,
    Modify,
    BodyOps,
    Datums,
    History,
    Inspect,
    Print,
}

impl FocusPack {
    pub const ALL: [FocusPack; 10] = [
        FocusPack::Document,
        FocusPack::Assembly,
        FocusPack::Sketch,
        FocusPack::Solid,
        FocusPack::Modify,
        FocusPack::BodyOps,
        FocusPack::Datums,
        FocusPack::History,
        FocusPack::Inspect,
        FocusPack::Print,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FocusPack::Document => "document",
            FocusPack::Assembly => "assembly",
            FocusPack::Sketch => "sketch",
            FocusPack::Solid => "solid",
            FocusPack::Modify => "modify",
            FocusPack::BodyOps => "body_ops",
            FocusPack::Datums => "datums",
            FocusPack::History => "history",
            FocusPack::Inspect => "inspect",
            FocusPack::Print => "print",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "document" => Some(FocusPack::Document),
            "assembly" => Some(FocusPack::Assembly),
            "sketch" => Some(FocusPack::Sketch),
            "solid" => Some(FocusPack::Solid),
            "modify" => Some(FocusPack::Modify),
            "body_ops" => Some(FocusPack::BodyOps),
            "datums" => Some(FocusPack::Datums),
            "history" => Some(FocusPack::History),
            "inspect" => Some(FocusPack::Inspect),
            "print" => Some(FocusPack::Print),
            _ => None,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            FocusPack::Document => "Document name, project export/load, and session metadata.",
            FocusPack::Assembly => {
                "Assembly components, occurrences, poses, grounding, and solution inspect."
            }
            FocusPack::Sketch => {
                "Sketch creation, constraints, dimensions, and sketch modify tools."
            }
            FocusPack::Solid => "Solid creators: extrude, revolve, sweep, loft, and rib.",
            FocusPack::Modify => "Edge and face modifiers: fillet, chamfer, and hole.",
            FocusPack::BodyOps => "Body operations: shell, move/copy, mirror, patterns, combine, split, and STEP import.",
            FocusPack::Datums => "Construction planes and datum features.",
            FocusPack::History => "Rollback, delete, and reorder in feature history.",
            FocusPack::Inspect => "Read-only solid and sketch definition catalogs.",
            FocusPack::Print => {
                "Manufacturing export: 3MF/STL/STEP, materials, appearance, and print demos."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisclosureMode {
    Dynamic,
    FullStatic,
}

impl DisclosureMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DisclosureMode::Dynamic => "dynamic",
            DisclosureMode::FullStatic => "full_static",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dynamic" => Some(DisclosureMode::Dynamic),
            "full_static" => Some(DisclosureMode::FullStatic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvertisementState {
    Active,
    Soft,
    HiddenButCallable,
}

impl AdvertisementState {
    pub fn as_str(self) -> &'static str {
        match self {
            AdvertisementState::Active => "active",
            AdvertisementState::Soft => "soft",
            AdvertisementState::HiddenButCallable => "hidden_but_callable",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SoftEntry {
    expires_at_ms: u64,
    last_touched_ms: u64,
}

#[derive(Debug, Clone)]
pub struct DisclosureState {
    mode: DisclosureMode,
    active: FocusPack,
    soft: HashMap<FocusPack, SoftEntry>,
    soft_order: Vec<FocusPack>,
    explicit_focus: Option<FocusPack>,
    pending_notify_at_ms: Option<u64>,
    now_ms: u64,
}

impl Default for DisclosureState {
    fn default() -> Self {
        Self::new()
    }
}

impl DisclosureState {
    pub fn new() -> Self {
        Self {
            mode: DisclosureMode::Dynamic,
            active: FocusPack::Document,
            soft: HashMap::new(),
            soft_order: Vec::new(),
            explicit_focus: None,
            pending_notify_at_ms: None,
            now_ms: Self::wall_clock_ms(),
        }
    }

    fn wall_clock_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    fn now(&self) -> u64 {
        self.now_ms
    }

    #[cfg(test)]
    pub fn set_clock_for_test(now_ms: u64) {
        TEST_CLOCK.with(|cell| cell.set(Some(now_ms)));
    }

    #[cfg(test)]
    pub fn advance_for_test(delta_ms: u64) {
        TEST_CLOCK.with(|cell| {
            let next = cell.get().unwrap_or(0).saturating_add(delta_ms);
            cell.set(Some(next));
        });
    }

    #[cfg(test)]
    fn test_now() -> Option<u64> {
        TEST_CLOCK.with(|cell| cell.get())
    }

    pub fn set_mode(&mut self, mode: DisclosureMode) {
        self.refresh_clock();
        self.mode = mode;
        self.schedule_notify(true);
    }

    pub fn set_focus(&mut self, focus: FocusPack, explicit: bool) {
        self.refresh_clock();
        if explicit {
            self.explicit_focus = Some(focus);
        }
        if focus == self.active {
            self.soft.remove(&focus);
            self.soft_order.retain(|pack| *pack != focus);
            self.schedule_notify(false);
            return;
        }
        let previous = self.active;
        // Move active first so mark_soft does not no-op on previous==active.
        self.active = focus;
        self.soft.remove(&focus);
        self.soft_order.retain(|pack| *pack != focus);
        self.mark_soft(previous);
        self.enforce_soft_lru();
        self.schedule_notify(false);
    }

    pub fn auto_hint(&mut self, focus: FocusPack) {
        if self.explicit_focus.is_some() {
            return;
        }
        self.set_focus(focus, false);
    }

    pub fn clear_explicit_lock(&mut self) {
        self.explicit_focus = None;
    }

    pub fn active(&self) -> FocusPack {
        self.active
    }

    pub fn re_promote(&mut self, pack: FocusPack) {
        self.refresh_clock();
        if pack == self.active {
            return;
        }
        let now = self.now();
        self.soft.insert(
            pack,
            SoftEntry {
                expires_at_ms: now.saturating_add(SOFT_REPROMOTE_MS),
                last_touched_ms: now,
            },
        );
        self.touch_soft_order(pack);
        self.enforce_soft_lru();
        if self.mode == DisclosureMode::Dynamic {
            self.schedule_notify(false);
        }
    }

    pub fn tick_soft_expiry(&mut self) -> bool {
        self.refresh_clock();
        if self.mode == DisclosureMode::FullStatic {
            return false;
        }
        let now = self.now();
        let expired: Vec<FocusPack> = self
            .soft
            .iter()
            .filter_map(|(pack, entry)| {
                if entry.expires_at_ms <= now {
                    Some(*pack)
                } else {
                    None
                }
            })
            .collect();
        if expired.is_empty() {
            return false;
        }
        for pack in expired {
            self.soft.remove(&pack);
            self.soft_order.retain(|existing| *existing != pack);
        }
        self.schedule_notify(false);
        true
    }

    pub fn take_notify_if_due(&mut self) -> Option<Value> {
        self.refresh_clock();
        let now = self.now();
        let due = self
            .pending_notify_at_ms
            .is_some_and(|deadline| now >= deadline);
        if due {
            self.pending_notify_at_ms = None;
            Some(list_changed_notification())
        } else {
            None
        }
    }

    /// Earliest wall-clock deadline for a pending list_changed or soft-pack expiry.
    pub fn next_wake_at_ms(&self) -> Option<u64> {
        let mut wake = self.pending_notify_at_ms;
        if self.mode == DisclosureMode::Dynamic {
            for entry in self.soft.values() {
                wake = Some(match wake {
                    Some(existing) => existing.min(entry.expires_at_ms),
                    None => entry.expires_at_ms,
                });
            }
        }
        wake
    }

    /// Milliseconds until [`Self::next_wake_at_ms`], or `None` if nothing is scheduled.
    pub fn ms_until_wake(&mut self) -> Option<u64> {
        self.refresh_clock();
        let now = self.now();
        self.next_wake_at_ms()
            .map(|deadline| deadline.saturating_sub(now))
    }

    pub fn is_advertised(&self, tool_name: &str, pack: FocusPack, spine: bool) -> bool {
        if spine || self.mode == DisclosureMode::FullStatic {
            return true;
        }
        if pack == self.active {
            return true;
        }
        self.soft
            .get(&pack)
            .is_some_and(|entry| entry.expires_at_ms > self.now())
            && self.soft_order.contains(&pack)
            && !tool_name.is_empty()
    }

    pub fn advertisement_state(&self, pack: FocusPack, spine: bool) -> AdvertisementState {
        if spine || self.mode == DisclosureMode::FullStatic {
            return AdvertisementState::Active;
        }
        if pack == self.active {
            return AdvertisementState::Active;
        }
        if self
            .soft
            .get(&pack)
            .is_some_and(|entry| entry.expires_at_ms > self.now())
            && self.soft_order.contains(&pack)
        {
            return AdvertisementState::Soft;
        }
        AdvertisementState::HiddenButCallable
    }

    pub fn disclosure_note(&self, pack: FocusPack, spine: bool) -> Value {
        let state = self.advertisement_state(pack, spine);
        json!({
            "advertised": state != AdvertisementState::HiddenButCallable || self.mode == DisclosureMode::FullStatic,
            "pack": pack.as_str(),
            "state": state.as_str(),
            "mode": self.mode.as_str(),
            "active_focus": self.active.as_str(),
        })
    }

    pub fn status_json(&self) -> Value {
        let now = self.now();
        let soft: Vec<Value> = self
            .soft_order
            .iter()
            .filter_map(|pack| {
                self.soft.get(pack).map(|entry| {
                    json!({
                        "pack": pack.as_str(),
                        "expires_in_ms": entry.expires_at_ms.saturating_sub(now),
                        "last_touched_ms": entry.last_touched_ms,
                    })
                })
            })
            .collect();
        json!({
            "mode": self.mode.as_str(),
            "active_focus": self.active.as_str(),
            "explicit_focus": self.explicit_focus.map(|pack| pack.as_str()),
            "soft_packs": soft,
            "notify_pending_in_ms": self.pending_notify_at_ms.map(|deadline| deadline.saturating_sub(now)),
        })
    }

    pub fn focus_areas_json() -> Value {
        Value::Array(
            FocusPack::ALL
                .iter()
                .map(|pack| {
                    json!({
                        "id": pack.as_str(),
                        "description": pack.description(),
                    })
                })
                .collect(),
        )
    }

    fn refresh_clock(&mut self) {
        #[cfg(test)]
        if let Some(now) = Self::test_now() {
            self.now_ms = now;
            return;
        }
        self.now_ms = Self::wall_clock_ms();
    }

    fn mark_soft(&mut self, pack: FocusPack) {
        if pack == self.active {
            return;
        }
        let now = self.now();
        self.soft.insert(
            pack,
            SoftEntry {
                expires_at_ms: now.saturating_add(SOFT_TTL_MS),
                last_touched_ms: now,
            },
        );
        self.touch_soft_order(pack);
    }

    fn touch_soft_order(&mut self, pack: FocusPack) {
        self.soft_order.retain(|existing| *existing != pack);
        self.soft_order.push(pack);
    }

    fn enforce_soft_lru(&mut self) {
        while self.soft_order.len() > SOFT_LRU {
            if let Some(oldest) = self.soft_order.first().copied() {
                self.soft_order.remove(0);
                if oldest != self.active {
                    self.soft.remove(&oldest);
                }
            } else {
                break;
            }
        }
    }

    fn schedule_notify(&mut self, immediate: bool) {
        if self.mode == DisclosureMode::FullStatic {
            self.pending_notify_at_ms = Some(self.now());
            return;
        }
        let now = self.now();
        let deadline = if immediate {
            now
        } else {
            now.saturating_add(FOCUS_THROTTLE_MS)
        };
        self.pending_notify_at_ms = Some(
            self.pending_notify_at_ms
                .map(|existing| existing.max(deadline))
                .unwrap_or(deadline),
        );
    }
}

#[cfg(test)]
std::thread_local! {
    static TEST_CLOCK: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

pub fn list_changed_notification() -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/tools/list_changed"
    })
}

/// Primary focus pack and spine flag for every registered MCP tool.
pub fn tags_for_tool(name: &str) -> (FocusPack, bool) {
    let spine = matches!(
        name,
        "cad_document"
            | "assembly_document"
            | "solid_scene"
            | "solid_recompute"
            | "cad_get_focus"
            | "cad_set_focus"
            | "cad_list_focus_areas"
            | "cad_get_tool_disclosure_mode"
            | "cad_set_tool_disclosure_mode"
            | "cad_list_all_tools"
            | "cad_cancel_recompute"
            | "cad_list_sessions"
            | "cad_interface"
            | "cad_attach"
            | "cad_refresh"
            | "cad_detach"
            | "cad_script"
            | "cad_compare_solids"
            | "cad_submit"
            | "cad_await_apply"
            | "cad_session_status"
    );
    if spine {
        let pack = match name {
            "cad_document" | "cad_script" => FocusPack::Document,
            "assembly_document" => FocusPack::Assembly,
            "solid_scene" | "solid_recompute" | "cad_compare_solids" => FocusPack::Inspect,
            _ => FocusPack::Document,
        };
        return (pack, true);
    }

    let pack = match name {
        "cad_set_document_name"
        | "cad_project_model"
        | "cad_load_project_model"
        | "cad_new_project" => FocusPack::Document,
        "assembly_solution"
        | "assembly_create_component"
        | "assembly_update_component"
        | "assembly_create_occurrence"
        | "assembly_update_occurrence"
        | "assembly_set_occurrence_pose"
        | "assembly_set_occurrence_grounded"
        | "assembly_create_joint"
        | "assembly_delete_joint"
        | "assembly_set_joint_enabled"
        | "assembly_set_joint_motion"
        | "assembly_create_gear_relation"
        | "assembly_update_gear_relation"
        | "assembly_delete_gear_relation"
        | "assembly_update_joint" => FocusPack::Assembly,
        "sketch_begin"
        | "sketch_finish"
        | "sketch_edit"
        | "sketch_active"
        | "sketch_finished"
        | "sketch_profiles"
        | "sketch_add_line"
        | "sketch_add_line_locked"
        | "sketch_add_midpoint_line"
        | "sketch_add_point"
        | "sketch_add_rectangle"
        | "sketch_add_rectangle_locked"
        | "sketch_add_circle"
        | "sketch_add_circle_locked"
        | "sketch_add_arc_3pt"
        | "sketch_add_arc_center"
        | "sketch_add_slot"
        | "sketch_add_spline"
        | "sketch_add_constraint"
        | "sketch_add_constraints"
        | "sketch_add_dimension"
        | "sketch_edit_dimension"
        | "sketch_move_dimension"
        | "sketch_delete_dimension"
        | "sketch_fillet"
        | "sketch_chamfer"
        | "sketch_offset"
        | "sketch_trim"
        | "sketch_extend"
        | "sketch_break"
        | "sketch_mirror"
        | "sketch_rectangular_pattern"
        | "sketch_circular_pattern"
        | "sketch_move_copy"
        | "sketch_scale"
        | "sketch_polygon"
        | "sketch_move_point"
        | "sketch_toggle_fix"
        | "sketch_delete_entities"
        | "sketch_undo"
        | "sketch_redo"
        | "sketch_set_grid_snap"
        | "sketch_set_grid_step"
        | "sketch_eval_expression"
        | "sketch_set_dimension_style"
        | "sketch_preview_line"
        | "sketch_preview_line_locked"
        | "sketch_preview_fillet"
        | "sketch_preview_offset"
        | "sketch_preview_trim" => FocusPack::Sketch,
        "solid_extrude" | "solid_edit_extrude" | "solid_revolve" | "solid_edit_revolve"
        | "solid_sweep" | "solid_edit_sweep" | "solid_loft" | "solid_edit_loft" | "solid_rib"
        | "solid_edit_rib" => FocusPack::Solid,
        "solid_fillet"
        | "solid_edit_fillet"
        | "solid_chamfer"
        | "solid_edit_chamfer"
        | "solid_hole"
        | "solid_edit_hole"
        | "solid_external_thread"
        | "solid_edit_external_thread" => FocusPack::Modify,
        "solid_shell"
        | "solid_edit_shell"
        | "solid_mirror"
        | "solid_edit_mirror"
        | "solid_rectangular_pattern"
        | "solid_edit_rectangular_pattern"
        | "solid_circular_pattern"
        | "solid_edit_circular_pattern"
        | "solid_move_copy"
        | "solid_edit_move_copy"
        | "solid_combine"
        | "solid_edit_combine"
        | "solid_split_body"
        | "solid_edit_split_body"
        | "solid_import_step"
        | "solid_edit_import_step" => FocusPack::BodyOps,
        "construction_plane_definitions"
        | "construction_plane_offset"
        | "construction_plane_edit_offset"
        | "construction_plane_midplane"
        | "construction_plane_edit_midplane"
        | "construction_plane_at_angle"
        | "construction_plane_edit_at_angle" => FocusPack::Datums,
        "solid_set_rollback" | "solid_delete_feature" | "solid_reorder_feature" => {
            FocusPack::History
        }
        "solid_extrude_definitions"
        | "solid_revolve_definitions"
        | "solid_sweep_definitions"
        | "solid_loft_definitions"
        | "solid_rib_definitions"
        | "solid_fillet_definitions"
        | "solid_chamfer_definitions"
        | "solid_hole_definitions"
        | "solid_body_feature_definitions"
        | "solid_tessellate" => FocusPack::Inspect,
        "solid_export_step"
        | "solid_export_stl"
        | "solid_export_3mf"
        | "solid_export_preflight"
        | "material_catalog"
        | "body_appearances"
        | "set_body_appearance"
        | "demo_export_pip_3mf" => FocusPack::Print,
        _ => FocusPack::Document,
    };
    (pack, false)
}

pub fn auto_focus_for_tool(name: &str) -> Option<FocusPack> {
    if name.starts_with("sketch_") {
        return Some(if name == "sketch_finish" {
            FocusPack::Solid
        } else {
            FocusPack::Sketch
        });
    }
    if matches!(
        name,
        "solid_extrude"
            | "solid_edit_extrude"
            | "solid_revolve"
            | "solid_edit_revolve"
            | "solid_sweep"
            | "solid_edit_sweep"
            | "solid_loft"
            | "solid_edit_loft"
            | "solid_rib"
            | "solid_edit_rib"
    ) {
        return Some(FocusPack::Solid);
    }
    if matches!(
        name,
        "solid_fillet"
            | "solid_edit_fillet"
            | "solid_chamfer"
            | "solid_edit_chamfer"
            | "solid_hole"
            | "solid_edit_hole"
            | "solid_external_thread"
            | "solid_edit_external_thread"
    ) {
        return Some(FocusPack::Modify);
    }
    if name.starts_with("solid_")
        && (name.contains("shell")
            || name.contains("move_copy")
            || name.contains("mirror")
            || name.contains("pattern")
            || name.contains("combine")
            || name.contains("split_body")
            || name.contains("import_step"))
    {
        return Some(FocusPack::BodyOps);
    }
    if name.starts_with("construction_plane_") {
        return Some(FocusPack::Datums);
    }
    if matches!(
        name,
        "solid_set_rollback" | "solid_delete_feature" | "solid_reorder_feature"
    ) {
        return Some(FocusPack::History);
    }
    if name.ends_with("_definitions") || name == "solid_scene" || name == "solid_tessellate" {
        return Some(FocusPack::Inspect);
    }
    if matches!(
        name,
        "solid_export_step"
            | "solid_export_stl"
            | "solid_export_3mf"
            | "solid_export_preflight"
            | "material_catalog"
            | "body_appearances"
            | "set_body_appearance"
            | "demo_export_pip_3mf"
    ) {
        return Some(FocusPack::Print);
    }
    if matches!(
        name,
        "cad_set_document_name"
            | "cad_project_model"
            | "cad_load_project_model"
            | "cad_new_project"
    ) {
        return Some(FocusPack::Document);
    }
    if name.starts_with("assembly_") {
        return Some(FocusPack::Assembly);
    }
    None
}

/// Focus-mapping fixture for snapshot bridge tests.
/// Keep dialog keys aligned with `activeSolidDialog` in the desktop app.
#[cfg(test)]
pub fn focus_from_ui(
    mode: &str,
    active_tool: Option<&str>,
    solid_dialog: Option<&str>,
) -> FocusPack {
    if let Some(dialog) = solid_dialog {
        return match dialog {
            // Keep keys aligned with activeSolidDialog in the desktop app.
            "fillet" | "chamfer" | "hole" => FocusPack::Modify,
            "shell"
            | "mirror"
            | "rectangular_pattern"
            | "circular_pattern"
            | "combine"
            | "split_body"
            | "import_step"
            | "move_copy" => FocusPack::BodyOps,
            "extrude" | "revolve" | "sweep" | "loft" | "rib" => FocusPack::Solid,
            "construction_plane" | "offset_plane" | "midplane" | "plane_at_angle" => {
                FocusPack::Datums
            }
            _ => FocusPack::Solid,
        };
    }
    if let Some(tool) = active_tool {
        if tool.starts_with("sketch_") || tool == "pickPlane" {
            return FocusPack::Sketch;
        }
        if tool.starts_with("solid_") || tool.starts_with("construction_plane_") {
            return tags_for_tool(tool).0;
        }
    }
    match mode {
        "sketch" | "sketchEdit" => FocusPack::Sketch,
        "solid" | "feature" => FocusPack::Solid,
        "assembly" => FocusPack::Assembly,
        "modify" => FocusPack::Modify,
        "datums" | "pickPlane" => FocusPack::Datums,
        "history" => FocusPack::History,
        "inspect" => FocusPack::Inspect,
        "print" | "export" => FocusPack::Print,
        _ => FocusPack::Document,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_from_ui_matches_session_bridge_dialog_keys() {
        assert_eq!(
            focus_from_ui("solid", None, Some("construction_plane")),
            FocusPack::Datums
        );
        assert_eq!(
            focus_from_ui("solid", None, Some("fillet")),
            FocusPack::Modify
        );
        assert_eq!(focus_from_ui("sketch", None, None), FocusPack::Sketch);
    }

    #[test]
    fn soft_ttl_removes_pack_from_advertisement() {
        DisclosureState::set_clock_for_test(0);
        let mut state = DisclosureState::new();
        state.set_focus(FocusPack::Sketch, true);
        assert!(state.is_advertised("sketch_begin", FocusPack::Sketch, false));
        state.set_focus(FocusPack::Solid, true);
        assert!(state.is_advertised("sketch_begin", FocusPack::Sketch, false));
        DisclosureState::advance_for_test(SOFT_TTL_MS + 1);
        state.tick_soft_expiry();
        assert!(!state.is_advertised("sketch_begin", FocusPack::Sketch, false));
        assert_eq!(
            state.advertisement_state(FocusPack::Sketch, false),
            AdvertisementState::HiddenButCallable
        );
    }

    #[test]
    fn focus_notify_is_throttled() {
        DisclosureState::set_clock_for_test(0);
        let mut state = DisclosureState::new();
        state.set_focus(FocusPack::Sketch, true);
        state.set_focus(FocusPack::Solid, true);
        state.set_focus(FocusPack::Modify, true);
        assert!(state.take_notify_if_due().is_none());
        DisclosureState::advance_for_test(FOCUS_THROTTLE_MS);
        assert!(state.take_notify_if_due().is_some());
        assert!(state.take_notify_if_due().is_none());
    }

    #[test]
    fn full_static_advertises_everything() {
        let mut state = DisclosureState::new();
        state.set_mode(DisclosureMode::FullStatic);
        assert!(state.is_advertised("solid_extrude", FocusPack::Solid, false));
        assert_eq!(
            state.advertisement_state(FocusPack::Sketch, false),
            AdvertisementState::Active
        );
    }

    #[test]
    fn tags_cover_all_modeling_tools() {
        let modeling = [
            "cad_set_document_name",
            "cad_project_model",
            "cad_load_project_model",
            "cad_new_project",
            "sketch_begin",
            "sketch_finish",
            "sketch_edit",
            "sketch_active",
            "sketch_finished",
            "sketch_profiles",
            "sketch_add_line",
            "sketch_add_line_locked",
            "sketch_add_midpoint_line",
            "sketch_add_point",
            "sketch_add_rectangle",
            "sketch_add_rectangle_locked",
            "sketch_add_circle",
            "sketch_add_circle_locked",
            "sketch_add_arc_3pt",
            "sketch_add_arc_center",
            "sketch_add_slot",
            "sketch_add_spline",
            "sketch_add_constraint",
            "sketch_add_constraints",
            "sketch_add_dimension",
            "sketch_edit_dimension",
            "sketch_move_dimension",
            "sketch_delete_dimension",
            "sketch_fillet",
            "sketch_chamfer",
            "sketch_offset",
            "sketch_trim",
            "sketch_extend",
            "sketch_break",
            "sketch_mirror",
            "sketch_rectangular_pattern",
            "sketch_circular_pattern",
            "sketch_move_copy",
            "sketch_scale",
            "sketch_polygon",
            "sketch_move_point",
            "sketch_toggle_fix",
            "sketch_delete_entities",
            "sketch_undo",
            "sketch_redo",
            "sketch_set_grid_snap",
            "sketch_set_grid_step",
            "sketch_eval_expression",
            "sketch_set_dimension_style",
            "sketch_preview_line",
            "sketch_preview_line_locked",
            "sketch_preview_fillet",
            "sketch_preview_offset",
            "sketch_preview_trim",
            "solid_extrude",
            "solid_edit_extrude",
            "solid_revolve",
            "solid_edit_revolve",
            "solid_sweep",
            "solid_edit_sweep",
            "solid_loft",
            "solid_edit_loft",
            "solid_rib",
            "solid_edit_rib",
            "solid_fillet",
            "solid_edit_fillet",
            "solid_chamfer",
            "solid_edit_chamfer",
            "solid_hole",
            "solid_edit_hole",
            "solid_shell",
            "solid_edit_shell",
            "solid_mirror",
            "solid_edit_mirror",
            "solid_rectangular_pattern",
            "solid_edit_rectangular_pattern",
            "solid_circular_pattern",
            "solid_edit_circular_pattern",
            "solid_move_copy",
            "solid_edit_move_copy",
            "solid_combine",
            "solid_edit_combine",
            "solid_split_body",
            "solid_edit_split_body",
            "solid_import_step",
            "solid_edit_import_step",
            "construction_plane_definitions",
            "construction_plane_offset",
            "construction_plane_edit_offset",
            "construction_plane_midplane",
            "construction_plane_edit_midplane",
            "construction_plane_at_angle",
            "construction_plane_edit_at_angle",
            "solid_set_rollback",
            "solid_delete_feature",
            "solid_reorder_feature",
            "solid_extrude_definitions",
            "solid_revolve_definitions",
            "solid_sweep_definitions",
            "solid_loft_definitions",
            "solid_rib_definitions",
            "solid_fillet_definitions",
            "solid_chamfer_definitions",
            "solid_hole_definitions",
            "solid_body_feature_definitions",
            "solid_tessellate",
            "assembly_document",
            "assembly_solution",
            "assembly_create_component",
            "assembly_update_component",
            "assembly_create_occurrence",
            "assembly_update_occurrence",
            "assembly_set_occurrence_pose",
            "assembly_set_occurrence_grounded",
            "assembly_create_joint",
            "assembly_update_joint",
            "cad_document",
            "solid_scene",
            "solid_recompute",
        ];
        assert_eq!(modeling.len(), 119);
        for name in modeling {
            let (pack, spine) = tags_for_tool(name);
            assert!(
                !matches!(pack, FocusPack::Document) || name.starts_with("cad_") || spine,
                "unexpected default pack for {name}"
            );
            if name.starts_with("assembly_") {
                assert_eq!(pack, FocusPack::Assembly, "{name}");
            }
            let _ = spine;
        }
        for name in [
            "solid_export_3mf",
            "solid_export_stl",
            "solid_export_step",
            "solid_export_preflight",
            "material_catalog",
            "body_appearances",
            "set_body_appearance",
            "demo_export_pip_3mf",
        ] {
            assert_eq!(tags_for_tool(name).0, FocusPack::Print, "{name}");
        }
    }
}
