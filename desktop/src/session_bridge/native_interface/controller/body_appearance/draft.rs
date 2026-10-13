//! Edits the existing per-body manufacturing appearance. Untouched metadata
//! remains exact; catalog profiles come from the same shared export catalog.
use limo_cad_core::{BodyAppearance, Rgba8};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Field {
    Brand,
    Preset,
    FilamentType,
    Color,
    ColorName,
    MaterialName,
}

#[derive(Clone)]
pub(super) struct Draft {
    pub original: BodyAppearance,
    pub value: BodyAppearance,
}

impl Draft {
    pub fn new(appearance: BodyAppearance) -> Self {
        Self {
            original: appearance.clone(),
            value: appearance,
        }
    }
    pub fn dirty(&self) -> bool {
        self.original != self.value
    }

    pub fn text(&self, field: Field) -> String {
        match field {
            Field::Brand => self.value.brand.clone(),
            Field::Preset => self.value.preset_id.clone().unwrap_or_default(),
            Field::FilamentType => self.value.filament_type.clone(),
            Field::Color => self.value.color.to_hex_rgb(),
            Field::ColorName => self.value.color_name.clone(),
            Field::MaterialName => self.value.material_name.clone(),
        }
    }

    pub fn edit(&mut self, field: Field, text: &str) -> Result<(), String> {
        let mut next = self.value.clone();
        if field == Field::Preset {
            if text.is_empty() {
                next.preset_id = None;
            } else {
                let preset = limo_cad_export::find_preset(text)
                    .ok_or("Choose a material from the catalog")?;
                if !preset.brand.eq_ignore_ascii_case(&next.brand) {
                    return Err("Choose a material for the selected brand".into());
                }
                next = preset.to_appearance(next.body_id);
            }
        } else {
            match field {
                Field::Brand => {
                    next.brand = text.trim().into();
                }
                Field::FilamentType => {
                    next.filament_type = text.trim().into();
                    if next.filament_type != self.value.filament_type {
                        next.material = None;
                        next.density_g_cm3 = None;
                        next.filament_id = None;
                    }
                }
                Field::Color => next.color = parse_color(text, next.color.a)?,
                Field::ColorName => next.color_name = text.into(),
                Field::MaterialName => {
                    if text.trim().is_empty() {
                        return Err("Enter a material name".into());
                    }
                    next.material_name = text.trim().into();
                }
                Field::Preset => unreachable!(),
            }
            if next != self.value {
                next.preset_id = None;
            }
        }
        self.value = next;
        Ok(())
    }
}

fn parse_color(text: &str, alpha: u8) -> Result<Rgba8, String> {
    let text = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if text.len() != 6 || !text.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Enter six hexadecimal color digits, such as #C82828".into());
    }
    let channel = |offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap();
    Ok(Rgba8 {
        r: channel(0),
        g: channel(2),
        b: channel(4),
        a: alpha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_core::BodyId;

    #[test]
    fn custom_color_preserves_manufacturing_metadata_and_alpha() {
        let mut appearance = limo_cad_export::find_preset("bambu.pla.basic.red")
            .unwrap()
            .to_appearance(BodyId(7));
        appearance.color.a = 153;
        let mut draft = Draft::new(appearance.clone());
        draft.edit(Field::Color, "#C82828").unwrap();
        assert!(
            !draft.dirty(),
            "Reentering the catalog color must not clear its profile"
        );
        draft.edit(Field::Color, "#12abEF").unwrap();
        let mut expected = appearance;
        expected.color = Rgba8 {
            r: 18,
            g: 171,
            b: 239,
            a: 153,
        };
        expected.preset_id = None;
        assert_eq!(draft.value, expected);
        for invalid in ["#12345", "#12345678", "#GG0000", "零件"] {
            assert!(draft.edit(Field::Color, invalid).is_err());
            assert_eq!(draft.value, expected);
        }
    }

    #[test]
    fn material_selection_uses_shared_catalog_and_retains_body_identity() {
        let mut draft = Draft::new(BodyAppearance::default_for(BodyId(9)));
        assert!(draft.edit(Field::Preset, "bambu.pla.basic.red").is_err());
        draft.edit(Field::Brand, "Bambu Lab").unwrap();
        draft.edit(Field::Preset, "bambu.pla.basic.red").unwrap();
        assert_eq!(
            draft.value,
            limo_cad_export::find_preset("bambu.pla.basic.red")
                .unwrap()
                .to_appearance(BodyId(9))
        );
        let before = draft.value.clone();
        assert!(draft.edit(Field::Preset, "missing").is_err());
        assert_eq!(draft.value, before);
        draft.edit(Field::MaterialName, "Custom blend").unwrap();
        assert_eq!(draft.value.preset_id, None);
        assert_eq!(draft.value.filament_id, before.filament_id);
        assert_eq!(draft.value.density_g_cm3, before.density_g_cm3);
    }

    #[test]
    fn changing_family_clears_properties_and_vendor_facts_for_the_previous_material() {
        let appearance = limo_cad_export::find_preset("bambu.pla.basic.red")
            .unwrap()
            .to_appearance(BodyId(9));
        let mut draft = Draft::new(appearance.clone());
        draft.edit(Field::FilamentType, "PLA").unwrap();
        assert_eq!(draft.value, appearance);
        draft.edit(Field::FilamentType, "PETG").unwrap();
        assert!(draft.value.material.is_none());
        assert!(draft.value.preset_id.is_none());
        assert!(draft.value.filament_id.is_none());
        assert!(draft.value.density_g_cm3.is_none());
        assert_eq!(draft.value.body_id, BodyId(9));
    }

    #[test]
    fn non_filament_material_can_clear_brand_and_family_without_retaining_plastic_facts() {
        let appearance = limo_cad_export::find_preset("bambu.pla.basic.red")
            .unwrap()
            .to_appearance(BodyId(9));
        let mut draft = Draft::new(appearance.clone());
        draft.edit(Field::FilamentType, "").unwrap();
        draft.edit(Field::Brand, "").unwrap();
        draft
            .edit(Field::MaterialName, "Painted timber (visual designation)")
            .unwrap();
        let mut expected = appearance;
        expected.filament_type.clear();
        expected.brand.clear();
        expected.material_name = "Painted timber (visual designation)".into();
        expected.material = None;
        expected.density_g_cm3 = None;
        expected.filament_id = None;
        expected.preset_id = None;
        assert_eq!(draft.value, expected);
        let choices = super::super::choices(&draft, Field::Brand).unwrap();
        assert_eq!(
            choices
                .iter()
                .filter(|choice| choice.value.is_empty())
                .count(),
            1
        );
        assert_eq!(choices[0].label, "Unspecified");
    }

    #[test]
    fn legacy_curated_density_remains_visible_alongside_its_quality_note() {
        let draft = Draft::new(
            limo_cad_export::find_preset("prusa.pla.msasaki_orange")
                .unwrap()
                .to_appearance(BodyId(9)),
        );
        let rows = super::super::panel::property_rows(&draft);
        assert!(rows
            .iter()
            .any(|(_, name, value)| name.contains("Assigned density")
                && value.as_ref().is_some_and(|v| v.contains("g/cm^3"))));
        assert!(rows
            .iter()
            .any(|(_, name, _)| name.starts_with("Material data note")));
    }
}
