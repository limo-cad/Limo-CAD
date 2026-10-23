//! Grype ignore rules are release-gate exceptions. Each rule must equal one
//! reviewed scope exactly, every review expires, and both scans check this
//! immediately before applying the rules, so an exception cannot widen or
//! outlive its review.
use anyhow::{ensure, Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::{collections::BTreeSet, fs, path::Path};
use time::{Date, Month, OffsetDateTime};

pub const CONFIG: &str = ".github/grype/exceptions.yaml";
pub const REVIEWS: &str = ".github/grype/reviews.yaml";
pub const RECORD: &str = "docs/security/dependencies.md";
const MAX_REVIEW_DAYS: i64 = 90;
const TYPES: &[&str] = &["binary"];

/// The only package fields a rule may carry. Grype skips a blank condition and
/// treats regex-like names as patterns, so each field is required and literal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    name: String,
    version: String,
    #[serde(rename = "type")]
    kind: String,
    location: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    vulnerability: String,
    reason: String,
    package: Scope,
}

/// Unknown keys are rejected so the file cannot also set `exclude`, a lower
/// `fail-on-severity` or another option that hides findings.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    ignore: Vec<Rule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Review {
    vulnerability: String,
    package: Scope,
    owner: String,
    review_by: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reviews {
    reviews: Vec<Review>,
}

fn advisory(id: &str) -> Result<()> {
    let pattern = Regex::new(r"^(?:CVE-\d{4}-\d{4,}|GHSA(?:-[23456789cfghjmpqrvwx]{4}){3})$")?;
    ensure!(
        pattern.is_match(id),
        "advisory {id:?} is not a CVE or GHSA identifier"
    );
    Ok(())
}

fn scope(id: &str, scope: &Scope) -> Result<()> {
    let literal = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")?;
    ensure!(
        literal.is_match(&scope.name),
        "{id}: package name {:?} must be a literal name",
        scope.name
    );
    ensure!(
        Regex::new(r"^[0-9A-Za-z][0-9A-Za-z._~:-]*$")?.is_match(&scope.version),
        "{id}: package version {:?} must be a literal version",
        scope.version
    );
    ensure!(
        TYPES.contains(&scope.kind.as_str()),
        "{id}: package type {:?} is not one of {TYPES:?}",
        scope.kind
    );
    // A leading `**/` absorbs the scanner's temporary staging directory. The
    // file itself and at least one directory naming the package must be fixed.
    let segments: Vec<_> = scope
        .location
        .strip_prefix("**/")
        .with_context(|| format!("{id}: location must start with **/"))?
        .split('/')
        .collect();
    let wildcard = |segment: &str| segment.contains(['*', '?', '[', ']', '{', '}', '\\']);
    ensure!(
        segments.len() >= 2
            && segments
                .iter()
                .all(|segment| !segment.is_empty() && !segment.contains("**"))
            && !wildcard(segments[segments.len() - 1])
            && segments[..segments.len() - 1]
                .iter()
                .any(|segment| segment.chars().any(|c| c.is_ascii_alphanumeric())),
        "{id}: location {:?} must end in a literal file under a named directory",
        scope.location
    );
    Ok(())
}

fn review_date(id: &str, text: &str) -> Result<Date> {
    let parts = Regex::new(r"^(\d{4})-(\d{2})-(\d{2})$")?
        .captures(text)
        .with_context(|| format!("{id}: review-by {text:?} is not YYYY-MM-DD"))?;
    // The pattern admits only digits, so these parses cannot overflow.
    let (year, month, day): (i32, u8, u8) =
        (parts[1].parse()?, parts[2].parse()?, parts[3].parse()?);
    Month::try_from(month)
        .and_then(|month| Date::from_calendar_date(year, month, day))
        .with_context(|| format!("{id}: review-by {text:?} is not a calendar date"))
}

fn deadline(id: &str, text: &str, today: Date) -> Result<Date> {
    let date = review_date(id, text)?;
    ensure!(
        date >= today,
        "{id}: review expired on {date}; reassess before scanning"
    );
    ensure!(
        (date - today).whole_days() <= MAX_REVIEW_DAYS,
        "{id}: review-by {date} is more than {MAX_REVIEW_DAYS} days away"
    );
    Ok(date)
}

/// Returns each exception and its deadline, or the first reason to fail closed.
fn check(config: &str, reviews: &str, record: &str, today: Date) -> Result<Vec<(String, Date)>> {
    let config: Config = serde_yaml_ng::from_str(config).context(CONFIG)?;
    let reviews: Reviews = serde_yaml_ng::from_str(reviews).context(REVIEWS)?;
    let mut reviewed = BTreeSet::new();
    let mut result = Vec::new();
    let owner = Regex::new(r"^@[A-Za-z0-9-]+(?:/[A-Za-z0-9._-]+)?$")?;
    for review in &reviews.reviews {
        let id = &review.vulnerability;
        advisory(id)?;
        scope(id, &review.package)?;
        ensure!(
            owner.is_match(&review.owner),
            "{id}: owner {:?} must be a GitHub user or team",
            review.owner
        );
        let date = deadline(id, &review.review_by, today)?;
        ensure!(
            record.lines().any(|line| line.starts_with("| ")
                && line.contains(id.as_str())
                && line.contains(&review.owner)
                && line.contains(&format!("Review by **{}**", review.review_by))),
            "{id}: {RECORD} needs a row naming the advisory, {} and Review by **{}**",
            review.owner,
            review.review_by
        );
        ensure!(
            reviewed.insert((id.clone(), review.package.clone())),
            "{id}: duplicate review"
        );
        result.push((id.clone(), date));
    }
    let mut applied = BTreeSet::new();
    for rule in &config.ignore {
        let id = &rule.vulnerability;
        advisory(id)?;
        ensure!(
            !rule.reason.trim().is_empty(),
            "{id}: exception must give a reason"
        );
        let key = (id.clone(), rule.package.clone());
        ensure!(
            reviewed.contains(&key),
            "{id}: exception {:?} has no review with exactly that scope",
            rule.package
        );
        ensure!(applied.insert(key), "{id}: duplicate exception");
    }
    ensure!(
        applied == reviewed,
        "{REVIEWS} lists a review with no matching exception"
    );
    Ok(result)
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(args.count() == 0, "usage: cargo xtask advisory-exceptions");
    let root = super::root();
    let read = |file: &str| fs::read_to_string(Path::new(root).join(file)).context(file.to_owned());
    let today = OffsetDateTime::now_utc().date();
    let checked = check(&read(CONFIG)?, &read(REVIEWS)?, &read(RECORD)?, today)?;
    for (id, date) in &checked {
        println!("Advisory exception {id}: reviewed scope, valid through {date} (UTC)");
    }
    println!(
        "{} advisory exception(s) checked on {today}.",
        checked.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: Date = match Date::from_calendar_date(2026, Month::October, 23) {
        Ok(date) => date,
        Err(_) => panic!("test date"),
    };
    const RULE: &str = "ignore:\n  - vulnerability: CVE-2026-85091\n    reason: unreachable\n    package:\n      name: zlib\n      version: 1.3.2\n      type: binary\n      location: \"**/Limo-CAD-*-windows-*/z.dll\"\n";
    const REVIEW: &str = "reviews:\n  - vulnerability: CVE-2026-85091\n    package:\n      name: zlib\n      version: 1.3.2\n      type: binary\n      location: \"**/Limo-CAD-*-windows-*/z.dll\"\n    owner: \"@jackControls\"\n    review-by: \"2026-11-06\"\n";
    const ROW: &str =
        "| zlib | CVE-2026-85091 evidence | Owner: @jackControls. Review by **2026-11-06**. |\n";
    const LOCATION: &str = "\"**/Limo-CAD-*-windows-*/z.dll\"";

    fn rejects(result: Result<Vec<(String, Date)>>, reason: &str) {
        let error = format!("{:#}", result.expect_err(reason));
        assert!(error.contains(reason), "expected {reason:?}, got {error:?}");
    }

    #[test]
    fn repository_exceptions_are_reviewed_and_current() {
        let root = super::super::root();
        let read = |file| fs::read_to_string(root.join(file)).unwrap();
        // Use each review date as "today" so this stays a scope check after
        // expiry; the scans run the deadline check against the real date.
        let reviews: Reviews = serde_yaml_ng::from_str(&read(REVIEWS)).unwrap();
        for review in &reviews.reviews {
            let date = review_date(&review.vulnerability, &review.review_by).unwrap();
            check(&read(CONFIG), &read(REVIEWS), &read(RECORD), date).unwrap();
        }
    }

    #[test]
    fn reviewed_exception_passes_until_its_deadline() {
        assert_eq!(check(RULE, REVIEW, ROW, TODAY).unwrap().len(), 1);
        let last = Date::from_calendar_date(2026, Month::November, 6).unwrap();
        check(RULE, REVIEW, ROW, last).unwrap();
        rejects(
            check(RULE, REVIEW, ROW, last.next_day().unwrap()),
            "review expired on 2026-11-06",
        );
        check("ignore: []\n", "reviews: []\n", "", TODAY).unwrap();
    }

    #[test]
    fn broad_rules_fail_closed_even_when_reviewed() {
        // Applied to the rule and its review alike, so only scope validation
        // can reject them.
        let id = "vulnerability: CVE-2026-85091\n";
        for (from, to, reason) in [
            (id, "vulnerability: \"\"\n", "not a CVE or GHSA"),
            (id, "vulnerability: cve-2026-85091\n", "not a CVE or GHSA"),
            (id, "vulnerability: \"CVE-2026-*\"\n", "not a CVE or GHSA"),
            ("name: zlib", "name: \".*\"", "must be a literal name"),
            ("name: zlib", "name: \"zli+\"", "must be a literal name"),
            ("name: zlib", "name: \"\"", "must be a literal name"),
            (
                "version: 1.3.2",
                "version: \"1.3.*\"",
                "must be a literal version",
            ),
            (
                "version: 1.3.2",
                "version: \"\"",
                "must be a literal version",
            ),
            ("type: binary", "type: \"\"", "is not one of"),
            ("type: binary", "type: rust-crate", "is not one of"),
            (LOCATION, "\"\"", "must start with **/"),
            (LOCATION, "\"**\"", "must start with **/"),
            (LOCATION, "\"/tmp/Limo-CAD-x/z.dll\"", "must start with **/"),
            (
                LOCATION,
                "\"**/z.dll\"",
                "literal file under a named directory",
            ),
            (
                LOCATION,
                "\"**/*/z.dll\"",
                "literal file under a named directory",
            ),
            (
                LOCATION,
                "\"**/Limo-CAD-*/**/z.dll\"",
                "literal file under a named directory",
            ),
            (
                LOCATION,
                "\"**/Limo-CAD-*-windows-*/*.dll\"",
                "literal file under a named directory",
            ),
        ] {
            let (rule, review) = (RULE.replace(from, to), REVIEW.replace(from, to));
            assert!(rule != RULE && review != REVIEW, "{from}");
            rejects(check(&rule, &review, ROW, TODAY), reason);
        }
    }

    #[test]
    fn rules_that_differ_from_their_review_or_add_options_fail_closed() {
        let scope = "has no review with exactly that scope";
        let field = "unknown field";
        for (from, to, reason) in [
            ("version: 1.3.2", "version: 1.3.1", scope),
            ("name: zlib", "name: zlib1", scope),
            (LOCATION, "\"**/Limo-CAD-*-*/z.dll\"", scope),
            ("CVE-2026-85091", "CVE-2026-85092", scope),
            ("reason: unreachable", "reason: \" \"", "must give a reason"),
            (
                "reason: unreachable",
                "reason: x\n    include-aliases: true",
                field,
            ),
            (
                "reason: unreachable",
                "reason: x\n    fix-state: unknown",
                field,
            ),
            (
                "reason: unreachable",
                "reason: x\n    namespace: nvd:cpe",
                field,
            ),
            (
                "\n      location: ",
                "\n      upstream-name: zlib\n      location: ",
                field,
            ),
            (
                "\n      location: \"**/Limo-CAD-*-windows-*/z.dll\"",
                "",
                "missing field `location`",
            ),
        ] {
            let rule = RULE.replace(from, to);
            assert_ne!(rule, RULE, "{from}");
            rejects(check(&rule, REVIEW, ROW, TODAY), reason);
        }
        for extra in [
            "exclude:\n  - \"**/z.dll\"\n",
            "fail-on-severity: critical\n",
        ] {
            rejects(check(&format!("{RULE}{extra}"), REVIEW, ROW, TODAY), field);
        }
        let twice = format!("{RULE}{}", &RULE["ignore:\n".len()..]);
        rejects(check(&twice, REVIEW, ROW, TODAY), "duplicate exception");
        rejects(
            check("ignore: []\n", REVIEW, ROW, TODAY),
            "review with no matching exception",
        );
    }

    #[test]
    fn expired_malformed_distant_or_undocumented_reviews_fail_closed() {
        let date = "\"2026-11-06\"";
        let owner = "owner: \"@jackControls\"";
        for (from, to, reason) in [
            (date, "\"2026-10-22\"", "review expired on 2026-10-22"),
            (date, "\"2026-02-30\"", "is not a calendar date"),
            (date, "\"2026-13-01\"", "is not a calendar date"),
            (date, "\"2026-11-6\"", "is not YYYY-MM-DD"),
            (date, "\"next month\"", "is not YYYY-MM-DD"),
            (date, "\"2027-10-23\"", "more than 90 days away"),
            (owner, "owner: \"\"", "must be a GitHub user or team"),
            (owner, "owner: maintainers", "must be a GitHub user or team"),
            ("review-by", "review_by", "unknown field `review_by`"),
            (
                "    review-by: \"2026-11-06\"\n",
                "",
                "missing field `review-by`",
            ),
        ] {
            let review = REVIEW.replace(from, to);
            assert_ne!(review, REVIEW, "{from}");
            let row = ROW.replace("2026-11-06", to.trim_matches('"'));
            rejects(check(RULE, &review, &row, TODAY), reason);
        }
        let undocumented = "needs a row naming the advisory";
        for row in [
            String::new(),
            ROW.replace("2026-11-06", "2026-11-07"),
            ROW.replace("@jackControls", "@someone"),
            ROW.replace("CVE-2026-85091", "CVE-2026-85092"),
            ROW.trim_start_matches("| ").to_owned(),
        ] {
            rejects(check(RULE, REVIEW, &row, TODAY), undocumented);
        }
    }
}
