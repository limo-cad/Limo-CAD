use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::planner::CamPlanError;

const MAX_NBPOST_BYTES: usize = 2 * 1024 * 1024;

/// First compatibility target for callback-oriented JavaScript posts. This
/// list describes the planned v1 surface; no script is executed by the
/// analyzer.
const V1_CALLBACK_TARGET: &[&str] = &[
    "onCircular",
    "onClose",
    "onCommand",
    "onDwell",
    "onLinear",
    "onOpen",
    "onRapid",
    "onSection",
    "onSectionEnd",
    "onSpindleSpeed",
];

const REQUIRED_CALLBACKS: &[&str] = &["onOpen", "onSection", "onClose"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NbPostAnalysisRequestDto {
    pub file_name: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NbPostSourceKind {
    CallbackJavascript,
    UnknownJavascript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NbPostCompatibilityLevel {
    /// The source resembles a supported callback post, but execution remains
    /// intentionally disabled while the sandbox/runtime is built.
    AnalysisOnly,
    NotRecognized,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NbPostAnalysisDto {
    pub format: String,
    pub version: u32,
    pub file_name: String,
    pub source_bytes: usize,
    pub source_kind: NbPostSourceKind,
    pub compatibility: NbPostCompatibilityLevel,
    pub runnable: bool,
    pub callbacks: Vec<String>,
    pub callbacks_outside_v1_target: Vec<String>,
    pub missing_required_callbacks: Vec<String>,
    pub rights_notice_detected: bool,
    pub warnings: Vec<String>,
}

/// Inspect a user-selected `.nbpost` without evaluating it or persisting its
/// contents. Renaming a post changes only the local file association; it does
/// not convert the source or alter its copyright/license.
pub fn analyze_nbpost(
    request: &NbPostAnalysisRequestDto,
) -> Result<NbPostAnalysisDto, CamPlanError> {
    let file_name = request
        .file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    if file_name.is_empty() {
        return Err(CamPlanError("a .nbpost file name is required".to_string()));
    }
    if !file_name.to_ascii_lowercase().ends_with(".nbpost") {
        return Err(CamPlanError(
            "post files must use the .nbpost extension; renaming does not change the source license"
                .to_string(),
        ));
    }

    let source_bytes = request.source.len();
    if source_bytes == 0 {
        return Err(CamPlanError("the .nbpost source is empty".to_string()));
    }
    if source_bytes > MAX_NBPOST_BYTES {
        return Err(CamPlanError(format!(
            ".nbpost source exceeds the {} MiB analysis limit",
            MAX_NBPOST_BYTES / (1024 * 1024)
        )));
    }

    let callbacks = declared_callbacks(&request.source);
    let callback_set = callbacks
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let callback_shape = callback_set.contains("onOpen")
        && callback_set.contains("onSection")
        && callbacks.iter().any(|name| {
            matches!(
                name.as_str(),
                "onRapid" | "onLinear" | "onCircular" | "onCycle" | "onCyclePoint"
            )
        });
    let source_kind = if callback_shape {
        NbPostSourceKind::CallbackJavascript
    } else {
        NbPostSourceKind::UnknownJavascript
    };
    let compatibility = if callback_shape {
        NbPostCompatibilityLevel::AnalysisOnly
    } else {
        NbPostCompatibilityLevel::NotRecognized
    };
    let target = V1_CALLBACK_TARGET.iter().copied().collect::<BTreeSet<_>>();
    let callbacks_outside_v1_target = callbacks
        .iter()
        .filter(|name| !target.contains(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let missing_required_callbacks = REQUIRED_CALLBACKS
        .iter()
        .filter(|name| !callback_set.contains(**name))
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();

    let lowercase = request.source.to_ascii_lowercase();
    let rights_notice_detected = lowercase.contains("copyright")
        || lowercase.contains("all rights reserved")
        || lowercase.contains("spdx-license-identifier");
    let mut warnings = vec![
        "Analysis only: Limo CAD does not execute .nbpost scripts yet. The compatibility runtime and sandbox must fail closed before posts can generate NC code."
            .to_string(),
    ];
    if rights_notice_detected {
        warnings.push(
            "A rights or license notice was detected. Keep it intact and confirm your right to use the post; changing the extension does not change ownership or license terms."
                .to_string(),
        );
    }
    if !callback_shape {
        warnings.push(
            "The source does not expose the minimum section and motion callbacks expected by the compatibility scaffold."
                .to_string(),
        );
    }
    if !callbacks_outside_v1_target.is_empty() {
        warnings.push(format!(
            "{} callback(s) are outside the planned fixed 3-axis v1 surface and must remain unsupported until implemented and tested.",
            callbacks_outside_v1_target.len()
        ));
    }
    if !missing_required_callbacks.is_empty() {
        warnings.push(format!(
            "Missing expected lifecycle callback(s): {}.",
            missing_required_callbacks.join(", ")
        ));
    }

    Ok(NbPostAnalysisDto {
        format: "nbpost".to_string(),
        version: 1,
        file_name: file_name.to_string(),
        source_bytes,
        source_kind,
        compatibility,
        runnable: false,
        callbacks,
        callbacks_outside_v1_target,
        missing_required_callbacks,
        rights_notice_detected,
        warnings,
    })
}

fn declared_callbacks(source: &str) -> Vec<String> {
    let tokens = javascript_identifier_tokens(source);
    let mut callbacks = BTreeSet::new();
    for pair in tokens.windows(2) {
        if pair[0] == "function" && is_callback_identifier(&pair[1]) {
            callbacks.insert(pair[1].clone());
        }
    }
    callbacks.into_iter().collect()
}

fn is_callback_identifier(value: &str) -> bool {
    value.starts_with("on")
        && value.len() > 2
        && value.as_bytes()[2].is_ascii_uppercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Small non-executing lexer used only to avoid treating callback names inside
/// comments and string literals as declarations. This is intentionally not a
/// JavaScript parser or runtime.
fn javascript_identifier_tokens(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            quote @ (b'\'' | b'"' | b'`') => {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric()
                        || bytes[index] == b'_'
                        || bytes[index] == b'$')
                {
                    index += 1;
                }
                tokens.push(source[start..index].to_string());
            }
            _ => index += 1,
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_hand_authored_callback_nbpost_without_executing_it() {
        let analysis = analyze_nbpost(&NbPostAnalysisRequestDto {
            file_name: "shop-siemens.nbpost".to_string(),
            source: r#"
                // Copyright Example Shop. Used here only as a tiny test source.
                function onOpen() {}
                function onSection() {}
                function onRapid(x, y, z) {}
                function onLinear(x, y, z, feed) {}
                function onClose() {}
            "#
            .to_string(),
        })
        .unwrap();

        assert_eq!(analysis.format, "nbpost");
        assert_eq!(analysis.source_kind, NbPostSourceKind::CallbackJavascript);
        assert_eq!(
            analysis.compatibility,
            NbPostCompatibilityLevel::AnalysisOnly
        );
        assert!(!analysis.runnable);
        assert!(analysis.callbacks.contains(&"onLinear".to_string()));
        assert!(analysis.callbacks_outside_v1_target.is_empty());
        assert!(analysis.missing_required_callbacks.is_empty());
        assert!(analysis.rights_notice_detected);
    }

    #[test]
    fn ignores_callback_names_in_comments_and_strings() {
        let analysis = analyze_nbpost(&NbPostAnalysisRequestDto {
            file_name: "not-a-post.nbpost".to_string(),
            source: "// function onOpen() {}\nconst sample = 'function onSection() {}';"
                .to_string(),
        })
        .unwrap();

        assert!(analysis.callbacks.is_empty());
        assert_eq!(
            analysis.compatibility,
            NbPostCompatibilityLevel::NotRecognized
        );
    }

    #[test]
    fn reports_callbacks_beyond_the_fixed_axis_v1_target() {
        let analysis = analyze_nbpost(&NbPostAnalysisRequestDto {
            file_name: "advanced.nbpost".to_string(),
            source: r#"
                function onOpen() {}
                function onSection() {}
                function onLinear5D() {}
                function onCycle() {}
                function onRapid() {}
                function onClose() {}
            "#
            .to_string(),
        })
        .unwrap();

        assert_eq!(
            analysis.callbacks_outside_v1_target,
            vec!["onCycle".to_string(), "onLinear5D".to_string()]
        );
    }

    #[test]
    fn requires_the_deliberate_nbpost_extension() {
        let error = analyze_nbpost(&NbPostAnalysisRequestDto {
            file_name: "vendor.post".to_string(),
            source: "function onOpen() {}".to_string(),
        })
        .unwrap_err();
        assert!(error.to_string().contains(".nbpost extension"));
    }
}
