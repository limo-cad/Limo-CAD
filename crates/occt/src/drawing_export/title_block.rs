//! Bounded title-block typography, independent of the geometry kernel.
//!
//! Keep these conservative advances and the readable floor aligned with the
//! desktop title-block layout. Wrapping may normalize display whitespace;
//! every other character is retained, including an overlong Unicode word.

pub(super) const MIN_HEIGHT: f64 = 1.8;
pub(super) const LINE_SPACING: f64 = 1.2;

#[derive(Debug)]
pub(super) struct FittedText {
    pub height: f64,
    pub lines: Vec<String>,
}

fn advance(ch: char) -> f64 {
    match ch {
        ' ' => 0.33,
        'i' | 'l' | 'I' | '.' | ',' | ':' | ';' | '!' | '|' | '\'' | '`' => 0.32,
        'M' | 'W' | '@' | '%' => 0.95,
        ch if ch.is_ascii_uppercase() => 0.75,
        ch if ch.is_ascii() => 0.65,
        _ => 1.,
    }
}

pub(super) fn text_width(value: &str, height: f64) -> f64 {
    value.chars().map(advance).sum::<f64>() * height
}

fn wrap_paragraph(value: &str, height: f64, width: f64) -> Option<Vec<String>> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0.;
    for word in value.split_whitespace() {
        let word_width = text_width(word, height);
        if !line.is_empty() && used + height * advance(' ') + word_width <= width + 1e-9 {
            line.push(' ');
            line.push_str(word);
            used += height * advance(' ') + word_width;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            used = 0.;
        }
        for ch in word.chars() {
            let next = height * advance(ch);
            if next > width + 1e-9 {
                return None;
            }
            if !line.is_empty() && used + next > width + 1e-9 {
                lines.push(std::mem::take(&mut line));
                used = 0.;
            }
            line.push(ch);
            used += next;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    Some(lines)
}

fn wrap(value: &str, height: f64, width: f64) -> Option<Vec<String>> {
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    for paragraph in normalized.split('\n') {
        let wrapped = wrap_paragraph(paragraph, height, width)?;
        if wrapped.is_empty() {
            lines.push(String::new());
        } else {
            lines.extend(wrapped);
        }
    }
    Some(lines)
}

pub(super) fn fit_text(
    field: &str,
    value: &str,
    requested_height: f64,
    width: f64,
    available_height: f64,
) -> Result<FittedText, String> {
    if !requested_height.is_finite()
        || !width.is_finite()
        || !available_height.is_finite()
        || requested_height <= 0.
        || width <= 0.
        || available_height < MIN_HEIGHT
    {
        return Err(format!("Invalid title block cell for '{field}'"));
    }

    let mut height = requested_height.clamp(MIN_HEIGHT, 5.).min(available_height);
    loop {
        if let Some(lines) = wrap(value, height, width) {
            let block_height = if lines.is_empty() {
                0.
            } else {
                height + lines.len().saturating_sub(1) as f64 * height * LINE_SPACING
            };
            if block_height <= available_height + 1e-9 {
                return Ok(FittedText { height, lines });
            }
        }
        if height <= MIN_HEIGHT {
            break;
        }
        height = (height - 0.1).max(MIN_HEIGHT);
    }
    Err(format!(
        "Title block field '{field}' does not fit its {width:.1} × {available_height:.1} mm text area at the minimum readable height {MIN_HEIGHT} mm. Shorten the field or move the full detail to a drawing note."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_complete_words_and_normalizes_only_whitespace() {
        let text = "Manufacturing\tclearance\nreview";
        let fit = fit_text("title", text, 2.5, 29., 12.).unwrap();
        assert_eq!(fit.lines.join(" "), "Manufacturing clearance review");
        assert!(fit.lines.len() > 1);
        assert!(fit
            .lines
            .iter()
            .all(|line| text_width(line, fit.height) <= 29. + 1e-9));
    }

    #[test]
    fn splits_overlong_unicode_words_without_losing_characters() {
        let text = "軸受組立図面圧力確認";
        let fit = fit_text("sheet", text, 2.5, 9., 15.).unwrap();
        assert_eq!(fit.lines.concat(), text);
        assert!(fit
            .lines
            .iter()
            .all(|line| text_width(line, fit.height) <= 9. + 1e-9));
        assert!(fit.height >= MIN_HEIGHT);
    }

    #[test]
    fn explicit_paragraph_breaks_keep_their_vertical_space() {
        let fit = fit_text("title", "First\r\n\r\nSecond", 2., 177., 8.).unwrap();
        assert_eq!(fit.lines, ["First", "", "Second"]);
    }

    #[test]
    fn excessive_text_fails_instead_of_truncating_or_becoming_microscopic() {
        let error = fit_text("finish", &"W".repeat(4096), 2.5, 177., 5.).unwrap_err();
        assert!(error.contains("'finish'"));
        assert!(error.contains("minimum readable height 1.8 mm"));
    }

    #[test]
    fn height_floor_and_large_requested_height_are_bounded() {
        assert_eq!(
            fit_text("company", "ACME", 0.1, 177., 2.).unwrap().height,
            MIN_HEIGHT
        );
        assert_eq!(
            fit_text("company", "ACME", 1e100, 177., 2.).unwrap().height,
            2.
        );
        for invalid in [f64::NAN, f64::INFINITY, -1.] {
            assert!(fit_text("company", "ACME", invalid, 177., 2.).is_err());
        }
    }
}
