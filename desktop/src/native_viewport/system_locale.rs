//! Read the user's preferred UI language without changing platform settings.
//! Saved application preferences take precedence over this initial default.

use crate::app_preferences::{locale, Locale};

pub(crate) fn detected_locale() -> Locale {
    preferred_language()
        .as_deref()
        .map_or(Locale::En, locale::detect_language)
}

#[cfg(target_os = "windows")]
fn preferred_language() -> Option<String> {
    windows::preferred_language()
}

#[cfg(target_os = "macos")]
fn preferred_language() -> Option<String> {
    macos::preferred_language()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn preferred_language() -> Option<String> {
    environment_language(|name| std::env::var(name).ok())
}

/// LANGUAGE is the UI-specific preference list; the other values provide the
/// usual category/global fallbacks. This follows sys-locale's Unix priority.
/// Only the primary language is selected, as with navigator.language: an
/// unsupported first preference falls back to English, not a later language.
#[cfg(any(test, not(any(target_os = "windows", target_os = "macos"))))]
fn environment_language(mut read: impl FnMut(&str) -> Option<String>) -> Option<String> {
    for name in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        let Some(value) = read(name) else { continue };
        let language = if name == "LANGUAGE" {
            value
                .split(':')
                .map(str::trim)
                .find(|value| !value.is_empty())
        } else {
            let value = value.trim();
            (!value.is_empty()).then_some(value)
        };
        if let Some(language) = language {
            return Some(language.to_owned());
        }
    }
    None
}

#[cfg(any(test, target_os = "windows"))]
fn first_windows_language(languages: &[u16]) -> Option<String> {
    if languages.len() < 2 || !languages.ends_with(&[0, 0]) {
        return None;
    }
    let end = languages.iter().position(|unit| *unit == 0)?;
    (end > 0)
        .then(|| String::from_utf16(&languages[..end]).ok())
        .flatten()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::first_windows_language;

    const MUI_LANGUAGE_NAME: u32 = 8;
    const MAX_UI_LANGUAGE_CHARS: u32 = 32 * 1024;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserPreferredUILanguages(
            flags: u32,
            language_count: *mut u32,
            languages: *mut u16,
            buffer_length: *mut u32,
        ) -> i32;
    }

    pub(super) fn preferred_language() -> Option<String> {
        for _ in 0..3 {
            let mut count = 0;
            let mut length = 0;

            let success = unsafe {
                GetUserPreferredUILanguages(
                    MUI_LANGUAGE_NAME,
                    &mut count,
                    std::ptr::null_mut(),
                    &mut length,
                )
            };
            if success == 0 || !(2..=MAX_UI_LANGUAGE_CHARS).contains(&length) {
                return None;
            }
            let mut buffer = vec![0; length as usize];

            let success = unsafe {
                GetUserPreferredUILanguages(
                    MUI_LANGUAGE_NAME,
                    &mut count,
                    buffer.as_mut_ptr(),
                    &mut length,
                )
            };
            if success != 0 {
                return (count > 0)
                    .then(|| {
                        buffer
                            .get(..length as usize)
                            .and_then(first_windows_language)
                    })
                    .flatten();
            }
        }
        None
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_char, c_void};

    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFLocaleCopyPreferredLanguages() -> *const c_void;
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        fn CFStringGetCString(
            string: *const c_void,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRelease(value: *const c_void);
    }

    struct Languages(*const c_void);
    impl Drop for Languages {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    pub(super) fn preferred_language() -> Option<String> {
        let pointer = unsafe { CFLocaleCopyPreferredLanguages() };
        if pointer.is_null() {
            return None;
        }
        let languages = Languages(pointer);

        let language = unsafe {
            if CFArrayGetCount(languages.0) <= 0 {
                return None;
            }
            CFArrayGetValueAtIndex(languages.0, 0)
        };
        if language.is_null() {
            return None;
        }
        let mut utf8 = [0_u8; 256];

        let success = unsafe {
            CFStringGetCString(
                language,
                utf8.as_mut_ptr().cast(),
                utf8.len() as isize,
                UTF8,
            )
        };
        if success == 0 {
            return None;
        }
        let end = utf8.iter().position(|byte| *byte == 0)?;
        std::str::from_utf8(&utf8[..end])
            .ok()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(values: &[(&str, &str)]) -> Locale {
        environment_language(|name| {
            values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        })
        .as_deref()
        .map_or(Locale::En, locale::detect_language)
    }

    #[test]
    fn linux_ui_preference_and_category_precedence_are_deterministic() {
        let choices = [
            ("LANGUAGE", "es_MX:de_DE"),
            ("LC_ALL", "zh_CN.UTF-8"),
            ("LC_MESSAGES", "de_DE.UTF-8"),
            ("LANG", "en_US.UTF-8"),
        ];
        assert_eq!(environment(&choices), Locale::Es);
        assert_eq!(environment(&choices[1..]), Locale::ZhCn);
        assert_eq!(environment(&choices[2..]), Locale::De);
        assert_eq!(environment(&choices[3..]), Locale::En);
        assert_eq!(
            environment(&[
                ("LANGUAGE", ""),
                ("LC_ALL", " "),
                ("LC_MESSAGES", "de_AT.UTF-8@euro")
            ]),
            Locale::De
        );
        assert_eq!(environment(&[("LANGUAGE", " : ZH_tw :es")]), Locale::ZhCn);
        assert_eq!(environment(&[]), Locale::En);
    }

    #[test]
    fn unsupported_primary_language_and_portable_locales_use_english() {
        assert_eq!(
            environment(&[("LANGUAGE", "fr_FR:de_DE"), ("LANG", "es_ES")]),
            Locale::En
        );
        assert_eq!(
            environment(&[("LC_ALL", "C"), ("LANG", "de_DE")]),
            Locale::En
        );
        assert_eq!(
            environment(&[("LC_MESSAGES", "POSIX"), ("LANG", "es_ES")]),
            Locale::En
        );
        assert_eq!(environment(&[("LANG", "C.UTF-8")]), Locale::En);
        assert_eq!(environment(&[("LANG", "zh_Hans_CN.UTF-8")]), Locale::ZhCn);
    }

    #[test]
    fn windows_multisz_uses_primary_language_and_rejects_invalid_buffers() {
        let buffer: Vec<u16> = "zh-CN\0en-US\0\0".encode_utf16().collect();
        assert_eq!(first_windows_language(&buffer).as_deref(), Some("zh-CN"));
        let unsupported: Vec<u16> = "fr-FR\0de-DE\0\0".encode_utf16().collect();
        assert_eq!(
            locale::detect_language(&first_windows_language(&unsupported).unwrap()),
            Locale::En
        );
        for invalid in [
            vec![],
            vec![0],
            vec![0, 0],
            vec![b'e' as u16, 0],
            vec![0xd800, 0, 0],
        ] {
            assert!(first_windows_language(&invalid).is_none(), "{invalid:?}");
        }
    }
}
