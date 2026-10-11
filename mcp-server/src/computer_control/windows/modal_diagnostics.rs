//! Bounded refusal evidence, never native-window admission or input authority.
use serde_json::{json, Value};

pub(super) const MAX_WINDOWS: usize = 64;

pub(super) struct CandidateFacts {
    pub hwnd: usize,
    pub pid: u32,
    pub owner_hwnd: Option<usize>,
    pub class: Option<String>,
    pub valid: bool,
    pub owned_dialog: bool,
    pub visible: bool,
    pub enabled: bool,
    pub iconic: bool,
}

impl CandidateFacts {
    fn snapshot(&self, main: usize, pid: u32) -> Value {
        let same_process = pid != 0 && self.pid == pid;
        let mut rejections = Vec::new();
        if !self.valid {
            rejections.push("invalid_window");
        }
        if self.hwnd == main {
            rejections.push("main_window_not_dialog");
        }
        if !same_process {
            rejections.push("not_same_process");
        } else {
            if !self.owned_dialog {
                rejections.push("no_same_process_chain_to_published_main");
            }
            if !self.visible {
                rejections.push("not_visible");
            }
            if !self.enabled {
                rejections.push("not_enabled");
            }
            if self.iconic {
                rejections.push("minimized");
            }
            match self.class.as_deref() {
                Some("#32770") => {}
                Some(_) => rejections.push("not_standard_dialog_class"),
                None => rejections.push("dialog_class_unavailable"),
            }
        }
        json!({"hwnd":self.hwnd,"pid":self.pid,"same_process":same_process,
            "owner_hwnd":same_process.then_some(self.owner_hwnd).flatten(),
            "class":same_process.then_some(self.class.as_deref()).flatten(),
            "visible":same_process.then_some(self.visible),
            "enabled":same_process.then_some(self.enabled),
            "iconic":same_process.then_some(self.iconic),
            "owned_dialog":same_process.then_some(self.owned_dialog),
            "rejections":rejections})
    }
}

pub(super) fn blocked(
    main: usize,
    pid: u32,
    candidates: &[(&str, CandidateFacts)],
    windows: &[CandidateFacts],
    truncated: bool,
    enumeration_error: Option<String>,
) -> String {
    json!({"code":"computer_control_modal_target_unqualified",
        "message":"CAD is blocked by a modal window without a qualified native target",
        "target_discovery_sent_input":false,"diagnostics_only":true,"main_hwnd":main,"main_pid":pid,
        "candidates":candidates.iter().take(2).map(|(source, facts)|
            json!({"source":source,"facts":facts.snapshot(main,pid)})).collect::<Vec<_>>(),
        "same_process_top_level_windows":windows.iter().filter(|facts|facts.pid==pid && pid!=0)
            .take(MAX_WINDOWS).map(|facts|facts.snapshot(main,pid)).collect::<Vec<_>>(),
        "windows_truncated":truncated || windows.len()>MAX_WINDOWS,
        "enumeration_error":enumeration_error,
        "hint":"Read-only refusal facts only. This diagnostic grants no class/owner exception and performs no focus activation or input. Observe freshly after the cause is resolved."})
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(hwnd: usize) -> CandidateFacts {
        CandidateFacts {
            hwnd,
            pid: 7,
            owner_hwnd: Some(1),
            class: Some("#32770".into()),
            valid: true,
            owned_dialog: true,
            visible: true,
            enabled: true,
            iconic: false,
        }
    }

    #[test]
    fn blocked_modal_refusal_distinguishes_class_and_owner_without_foreign_text_or_authority() {
        let mut class_mismatch = candidate(2);
        class_mismatch.class = Some("OwnedPropertySheet".into());
        let mut foreign = candidate(3);
        foreign.pid = 8;
        foreign.class = Some("foreign class must not be exposed".into());
        let mut windows: Vec<_> = (10..80).map(candidate).collect();
        windows[0].owned_dialog = false;
        windows[1].enabled = false;
        windows.push(candidate(3));
        windows.last_mut().unwrap().pid = 8;
        let response = blocked(
            1,
            7,
            &[
                ("foreground_root", foreign),
                ("last_active_popup", class_mismatch),
            ],
            &windows,
            false,
            Some("enumeration failed".into()),
        );
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["target_discovery_sent_input"], false);
        assert!(value.get("input_sent").is_none());
        assert_eq!(value["diagnostics_only"], true);
        assert_eq!(value["code"], "computer_control_modal_target_unqualified");
        assert_eq!(
            value["candidates"][0]["facts"]["rejections"],
            json!(["not_same_process"])
        );
        assert!(value["candidates"][0]["facts"]["class"].is_null());
        assert!(value["candidates"][0]["facts"]["owner_hwnd"].is_null());
        assert_eq!(
            value["candidates"][1]["facts"]["rejections"],
            json!(["not_standard_dialog_class"])
        );
        assert_eq!(
            value["same_process_top_level_windows"]
                .as_array()
                .unwrap()
                .len(),
            MAX_WINDOWS
        );
        assert_eq!(
            value["same_process_top_level_windows"][0]["rejections"],
            json!(["no_same_process_chain_to_published_main"])
        );
        assert_eq!(
            value["same_process_top_level_windows"][1]["rejections"],
            json!(["not_enabled"])
        );
        assert_eq!(value["windows_truncated"], true);
        assert_eq!(value["enumeration_error"], "enumeration failed");
        assert!(!response.contains("foreign class"));
        assert!(!response.contains("caption"));
        assert!(value.get("observation").is_none());
        assert!(value.get("target_kind").is_none());
    }
}
