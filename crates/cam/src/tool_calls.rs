//! Tool calls come from the project tool library. The post may select numeric
//! or exact named calls; it never rewrites a label or substitutes an internal id.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::model::{CamToolCallMode, CamToolDto};
use crate::{CamCommandDto, CamDocumentDto, CamPlanError, CamProgramDto, CamSetupDto};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CamMachineToolCallDto {
    Name {
        name: String,
    },
    /// Numeric library selection. With active Siemens tool
    /// management this can denote a magazine location, not an internal T id.
    Number {
        number: u32,
    },
}

pub(crate) fn library_tool_call(
    tool: &CamToolDto,
    mode: CamToolCallMode,
    names: bool,
) -> Result<CamMachineToolCallDto, String> {
    let call = match (mode, tool.number) {
        (CamToolCallMode::Name, _) if names => CamMachineToolCallDto::Name { name: tool.name.clone() },
        (CamToolCallMode::Name, _) => return Err("This post requires numeric tool calls; select numbers and assign them in the project tool library.".into()),
        (_, Some(number)) => CamMachineToolCallDto::Number { number },
        (CamToolCallMode::Automatic, None) if names => CamMachineToolCallDto::Name { name: tool.name.clone() },
        _ => return Err(format!("Tool '{}' needs a number in the project tool library for this post.", tool.name)),
    };
    call.validate()
        .map_err(|e| format!("Project tool '{}': {e}", tool.name))?;
    Ok(call)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CamMachineToolBindingDto {
    pub tool_id: u64,
    pub call: CamMachineToolCallDto,
}

impl CamMachineToolCallDto {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Name { name } => {
                if name.is_empty()
                    || name.len() > 31
                    || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    return Err("Controller tool name must be 1–31 ASCII letters, digits or underscores, exactly as entered in the control. Spaces and special characters are not supported; names are never rewritten.".into());
                }
            }
            Self::Number { number } if *number == 0 || *number > 99_999_999 => {
                return Err("Controller tool number must be between 1 and 99999999; verify whether the control interprets it as a magazine location or tool number.".into());
            }
            _ => {}
        }
        Ok(())
    }

    pub fn siemens_word(&self) -> Result<String, String> {
        self.validate()?;
        Ok(match self {
            Self::Name { name } => format!("T=\"{name}\""),
            Self::Number { number } => format!("T{number}"),
        })
    }
}

pub(crate) fn siemens_program_tool_calls(
    document: &CamDocumentDto,
    _setup: &CamSetupDto,
    program: &CamProgramDto,
    mode: CamToolCallMode,
) -> Result<BTreeMap<u64, String>, CamPlanError> {
    let mut calls = BTreeMap::new();
    let mut owners = BTreeMap::new();
    for command in &program.commands {
        if let CamCommandDto::ToolChange { tool_id, .. } = command {
            let tool = document
                .tool(*tool_id)
                .ok_or_else(|| CamPlanError("Post references a missing project tool".into()))?;
            let word = library_tool_call(tool, mode, true)
                .and_then(|c| c.siemens_word())
                .map_err(CamPlanError)?;
            if owners
                .insert(word.clone(), *tool_id)
                .is_some_and(|id| id != *tool_id)
            {
                return Err(CamPlanError(format!(
                    "Tool call {word} is duplicated in the project tool library"
                )));
            }
            calls.insert(*tool_id, word);
        }
    }
    Ok(calls)
}

pub(crate) fn resolve_siemens_tool(
    document: &CamDocumentDto,
    _setup: &CamSetupDto,
    call: &CamMachineToolCallDto,
) -> Result<u64, String> {
    call.validate()?;
    let matches = document
        .tools
        .iter()
        .filter(|tool| match call {
            CamMachineToolCallDto::Name { name } => &tool.name == name,
            CamMachineToolCallDto::Number { number } => tool.number == Some(*number),
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [tool] => Ok(tool.id),
        [] => Err(format!(
            "{} does not match a project-library tool number/name",
            call.siemens_word()?
        )),
        _ => Err(format!(
            "{} is ambiguous in the project tool library",
            call.siemens_word()?
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{plan_setup, post_setup, CamPostRequestDto, PostDialect};
    fn named(tool_id: u64, name: &str) -> CamMachineToolBindingDto {
        CamMachineToolBindingDto {
            tool_id,
            call: CamMachineToolCallDto::Name { name: name.into() },
        }
    }
    #[test]
    fn library_identity_is_authoritative_and_legacy_bindings_are_not_used() {
        let mut doc = crate::post::tests::document(PostDialect::Siemens828d);
        doc.setups[0].machine.as_mut().unwrap().tool_calls = vec![named(1, "Shop_EM6a")];
        doc.tools[0].name = "Current_EM6a".into();
        doc.tools[0].number = None;
        let posted = post_setup(
            &doc,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        assert!(posted.nc.lines().any(|s| s == "T=\"Current_EM6a\""));
        assert!(!posted.nc.contains("Shop_EM6a"));
        let setup = &doc.setups[0];
        assert_eq!(
            resolve_siemens_tool(&doc, setup, &named(1, "Current_EM6a").call).unwrap(),
            1
        );
        assert!(resolve_siemens_tool(&doc, setup, &named(1, "SHOP_EM6A").call).is_err());
        assert!(
            resolve_siemens_tool(&doc, setup, &CamMachineToolCallDto::Number { number: 1 })
                .is_err()
        );
    }
    #[test]
    fn unmapped_legacy_project_can_post_directly_from_the_library() {
        let mut doc = crate::post::tests::document(PostDialect::Siemens828d);
        doc.setups[0].machine.as_mut().unwrap().tool_calls.clear();
        assert!(plan_setup(&doc, 1).is_ok());
        assert!(post_setup(
            &doc,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None
            }
        )
        .is_ok());
        let mut json = serde_json::to_value(&doc).unwrap();
        json["setups"][0]["machine"]
            .as_object_mut()
            .unwrap()
            .remove("tool_calls");
        let reopened: CamDocumentDto = serde_json::from_value(json).unwrap();
        assert!(reopened.setups[0]
            .machine
            .as_ref()
            .unwrap()
            .tool_calls
            .is_empty());
        assert!(plan_setup(&reopened, 1).is_ok());
    }
    #[test]
    fn duplicate_calls_ids_and_unrepresentable_names_are_rejected_not_sanitized() {
        for name in [
            "",
            " EM6",
            "EM6 ",
            "EM 6",
            "EM-6",
            "Ø6",
            "Mill\"\nM30",
            &"X".repeat(32),
        ] {
            assert!(named(1, name).call.siemens_word().is_err(), "{name:?}");
        }
        assert!(named(1, &"X".repeat(31)).call.siemens_word().is_ok());
        let mut doc = crate::post::tests::document(PostDialect::Siemens828d);
        let mut other = doc.tools[0].clone();
        other.id = 2;
        other.number = Some(2);
        doc.tools[0].number = Some(1);
        doc.tools.push(other);
        doc.next_tool_id = 3;
        let mut program = plan_setup(&doc, 1).unwrap();
        let change = program
            .commands
            .iter()
            .find(|c| matches!(c, CamCommandDto::ToolChange { .. }))
            .unwrap()
            .clone();
        let mut second = change;
        if let CamCommandDto::ToolChange { tool_id, .. } = &mut second {
            *tool_id = 2;
        }
        program.commands.push(second);
        assert!(
            siemens_program_tool_calls(&doc, &doc.setups[0], &program, CamToolCallMode::Name)
                .is_err()
        );
        assert!(siemens_program_tool_calls(
            &doc,
            &doc.setups[0],
            &program,
            CamToolCallMode::Number
        )
        .is_ok());
        doc.tools[1].name = "DistinctName".into();
        assert!(
            siemens_program_tool_calls(&doc, &doc.setups[0], &program, CamToolCallMode::Name)
                .is_ok()
        );
    }
}
