use super::*;
use limo_cad_cam::{CamChainRefDto, CamHeightGeometryDto, CamOperationDto, WcsOriginSpecDto};

impl SketchManager {
    pub(super) fn rename_sketch(
        &mut self,
        feature_id: FeatureId,
        name: &str,
    ) -> Result<(), SessionError> {
        let index = self
            .finished
            .iter()
            .position(|sketch| sketch.feature_id == feature_id)
            .ok_or_else(|| SessionError::Solid("the retained sketch no longer exists".into()))?;
        if self
            .finished
            .iter()
            .enumerate()
            .any(|(other, sketch)| other != index && sketch.session.name() == name)
        {
            return Err(SessionError::Solid("Sketch names must be unique".into()));
        }
        let old = self.finished[index].session.name().to_owned();
        self.solids
            .rename_sketch_references(&old, name)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        self.finished[index].session.set_name(name);
        self.document.rename_sketch_node(&old, name);
        for hidden in &mut self.project_visibility.hidden_sketch_names {
            rename(hidden, &old, name);
        }
        rename_cam_references(&mut self.cam, &old, name);
        Ok(())
    }
}

fn rename(value: &mut String, old: &str, new: &str) {
    if value == old {
        value.clear();
        value.push_str(new);
    }
}

fn rename_chain(reference: &mut Option<CamChainRefDto>, old: &str, new: &str) {
    let Some(reference) = reference
        .as_mut()
        .filter(|reference| reference.source == CamChainSource::Sketch)
    else {
        return;
    };
    for key in &mut reference.keys {
        let Some((sketch, entity)) = key
            .strip_prefix("sketch:")
            .and_then(|key| key.rsplit_once(':'))
        else {
            continue;
        };
        if sketch == old {
            *key = format!("sketch:{new}:{entity}");
        }
    }
}

fn rename_cam_references(cam: &mut CamDocumentDto, old: &str, new: &str) {
    for setup in &mut cam.setups {
        if let WcsOriginSpecDto::SketchPoint { sketch, .. } = &mut setup.wcs_origin {
            rename(sketch, old, new);
        }
        for operation in &mut setup.operations {
            match operation {
                CamOperationDto::Contour2d { chain_ref, .. }
                | CamOperationDto::Pocket2d { chain_ref, .. } => {
                    rename_chain(chain_ref, old, new);
                }
                CamOperationDto::Chamfer2d {
                    chain_ref,
                    additional_chains,
                    ..
                } => {
                    rename_chain(chain_ref, old, new);
                    for chain in additional_chains {
                        rename_chain(&mut chain.chain_ref, old, new);
                    }
                }
                _ => {}
            }
        }
    }
    for heights in &mut cam.height_expressions {
        for expression in [
            Some(&mut heights.clearance),
            Some(&mut heights.retract),
            Some(&mut heights.feed),
            Some(&mut heights.top),
            heights.bottom.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(
                CamHeightGeometryDto::SketchPoint { sketch, .. }
                | CamHeightGeometryDto::SketchLine { sketch, .. },
            ) = &mut expression.geometry
            {
                rename(sketch, old, new);
            }
        }
    }
}
