//! Shared locale dictionaries with English fallback.

use std::sync::OnceLock;

use serde_json::Value;

use super::Locale;

pub(crate) const SUPPORTED: [Locale; 4] = [Locale::En, Locale::ZhCn, Locale::Es, Locale::De];

static EN: OnceLock<Result<Value, String>> = OnceLock::new();
static ZH_CN: OnceLock<Result<Value, String>> = OnceLock::new();
static ES: OnceLock<Result<Value, String>> = OnceLock::new();
static DE: OnceLock<Result<Value, String>> = OnceLock::new();

/// Exact persisted locale spelling; this deliberately does not accept aliases.
#[cfg(test)]
pub(crate) fn parse(value: &str) -> Option<Locale> {
    match value {
        "en" => Some(Locale::En),
        "zh-CN" => Some(Locale::ZhCn),
        "es" => Some(Locale::Es),
        "de" => Some(Locale::De),
        _ => None,
    }
}

pub(crate) fn code(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "en",
        Locale::ZhCn => "zh-CN",
        Locale::Es => "es",
        Locale::De => "de",
    }
}

/// Display names remain in their own languages in every UI locale.
pub(crate) fn native_name(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "English",
        Locale::ZhCn => "简体中文",
        Locale::Es => "Español",
        Locale::De => "Deutsch",
    }
}

/// Resolve the shared product locale from a host language hint.
/// The host decides when to consult the OS; a valid saved preference wins first.
pub(crate) fn detect_language(language: &str) -> Locale {
    let language = language.to_lowercase();
    if language.starts_with("zh") {
        Locale::ZhCn
    } else if language.starts_with("es") {
        Locale::Es
    } else if language.starts_with("de") {
        Locale::De
    } else {
        Locale::En
    }
}

/// Validate the embedded assets at initialization so an asset error can be
/// reported explicitly. Each dictionary is decoded at most once, including on
/// failure; normal string lookup neither allocates nor parses JSON.
pub(crate) fn validate_dictionaries() -> Result<(), &'static str> {
    for locale in SUPPORTED {
        dictionary(locale)?;
    }
    Ok(())
}

/// Active dictionary -> English -> original dotted key, including for keys
/// whose values are objects rather than strings. Empty strings are valid.
pub(crate) fn translate(locale: Locale, key: &str) -> &str {
    translate_from(dictionary(locale).ok(), dictionary(Locale::En).ok(), key)
}

fn dictionary(locale: Locale) -> Result<&'static Value, &'static str> {
    let (cache, source) = match locale {
        Locale::En => (&EN, include_str!("../../../assets/i18n/en.json")),
        Locale::ZhCn => (&ZH_CN, include_str!("../../../assets/i18n/zh-CN.json")),
        Locale::Es => (&ES, include_str!("../../../assets/i18n/es.json")),
        Locale::De => (&DE, include_str!("../../../assets/i18n/de.json")),
    };
    cache
        .get_or_init(|| {
            let value: Value = serde_json::from_str(source).map_err(|error| {
                format!("Invalid embedded {} dictionary: {error}", code(locale))
            })?;
            if !value.is_object() {
                return Err(format!(
                    "Embedded {} dictionary must be an object",
                    code(locale)
                ));
            }
            Ok(value)
        })
        .as_ref()
        .map_err(String::as_str)
}

fn lookup<'a>(dictionary: &'a Value, key: &str) -> Option<&'a str> {
    let mut node = dictionary;
    for part in key.split('.') {
        node = node.as_object()?.get(part)?;
    }
    node.as_str()
}

fn translate_from<'a>(
    active: Option<&'a Value>,
    english: Option<&'a Value>,
    key: &'a str,
) -> &'a str {
    active
        .and_then(|dict| lookup(dict, key))
        .or_else(|| english.and_then(|dict| lookup(dict, key)))
        .unwrap_or(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_codes_are_exact_and_native_names_preserve_unicode() {
        for (locale, value, name) in [
            (Locale::En, "en", "English"),
            (Locale::ZhCn, "zh-CN", "简体中文"),
            (Locale::Es, "es", "Español"),
            (Locale::De, "de", "Deutsch"),
        ] {
            assert_eq!(parse(value), Some(locale));
            assert_eq!(code(locale), value);
            assert_eq!(native_name(locale), name);
        }
        for invalid in [
            "", "EN", "zh-cn", "zh", "zh-TW", "es-MX", "de-DE", " en", "fr",
        ] {
            assert_eq!(parse(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn detection_matches_existing_language_prefix_rules_without_os_access() {
        for (language, expected) in [
            ("zh-CN", Locale::ZhCn),
            ("ZH-tw", Locale::ZhCn),
            ("zh_Hans_CN.UTF-8", Locale::ZhCn),
            ("es-MX", Locale::Es),
            ("ES", Locale::Es),
            ("de-DE", Locale::De),
            ("DE_at.UTF-8", Locale::De),
            ("en-GB", Locale::En),
            ("fr-FR", Locale::En),
            ("", Locale::En),
        ] {
            assert_eq!(detect_language(language), expected, "{language}");
        }
    }

    #[test]
    fn embedded_dictionaries_are_valid_cached_and_translate_real_ui_strings() {
        validate_dictionaries().unwrap();
        for (locale, settings, open_script) in [
            (Locale::En, "Settings", "Open Script…"),
            (Locale::ZhCn, "设置", "打开脚本…"),
            (Locale::Es, "Configuración", "Abrir script…"),
            (Locale::De, "Einstellungen", "Skript öffnen…"),
        ] {
            assert_eq!(translate(locale, "topbar.settings"), settings);
            assert_eq!(translate(locale, "topbar.openScript"), open_script);
            assert_eq!(
                translate(locale, "missing.translation.key"),
                "missing.translation.key"
            );
            assert_eq!(translate(locale, "topbar"), "topbar");
            assert!(std::ptr::eq(
                dictionary(locale).unwrap(),
                dictionary(locale).unwrap()
            ));
        }
    }

    #[test]
    fn incomplete_dictionary_falls_back_to_english_then_key_including_non_strings() {
        let english = serde_json::json!({
            "panel": { "title": "Settings", "new": "New setting", "invalid": "English value", "empty": "Fallback" }
        });
        let active = serde_json::json!({
            "panel": { "title": "设置", "invalid": 7, "empty": "" }
        });
        assert_eq!(
            translate_from(Some(&active), Some(&english), "panel.title"),
            "设置"
        );
        assert_eq!(
            translate_from(Some(&active), Some(&english), "panel.new"),
            "New setting"
        );
        assert_eq!(
            translate_from(Some(&active), Some(&english), "panel.invalid"),
            "English value"
        );
        assert_eq!(
            translate_from(Some(&active), Some(&english), "panel.empty"),
            ""
        );
        for key in [
            "panel.absent",
            "panel",
            "panel.title.child",
            "panel..title",
            "",
        ] {
            assert_eq!(translate_from(Some(&active), Some(&english), key), key);
        }
        assert_eq!(
            translate_from(None, Some(&english), "panel.title"),
            "Settings"
        );
        assert_eq!(translate_from(None, None, "panel.title"), "panel.title");
    }
}
