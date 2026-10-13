//! Pinned public-release media with bounded downloads and atomic staging.
use anyhow::{ensure, Context, Result};
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

const NAMES: [&str; 3] = [
    "bench-build-full.mp4",
    "vise-build-full.mp4",
    "turbine-build-full.mp4",
];
const MAX_MEDIA: u64 = 128 * 1024 * 1024;
const MAX_JSON: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub(super) struct Input {
    pub name: String,
    pub release: String,
    pub src: String,
    url: String,
}
struct Asset {
    input: Input,
    bytes: u64,
    sha256: String,
}
struct Download {
    reader: Box<dyn Read>,
    length: Option<u64>,
}
trait Network {
    fn open(&self, url: &str) -> Result<Download>;
}
struct Http {
    agent: ureq::Agent,
    api_token: Option<String>,
}
impl Http {
    fn token_for(&self, url: &str) -> Option<&str> {
        url.starts_with(&format!("{}/", crate::repository::api()))
            .then_some(self.api_token.as_deref())
            .flatten()
    }
}
impl Network for Http {
    fn open(&self, url: &str) -> Result<Download> {
        let mut request = self
            .agent
            .get(url)
            .header("Accept", "*/*")
            .header("User-Agent", "Limo-CAD-Pages-media");
        if let Some(token) = self.token_for(url) {
            request = request
                .header("Authorization", format!("Bearer {token}"))
                .config()
                .max_redirects(0)
                .build();
        }
        let response = request
            .call()
            .with_context(|| format!("download public showcase metadata or media: {url}"))?;
        let length = response
            .headers()
            .get("content-length")
            .map(|v| v.to_str()?.parse::<u64>().map_err(anyhow::Error::from))
            .transpose()?;
        Ok(Download {
            reader: Box::new(response.into_body().into_reader()),
            length,
        })
    }
}

pub(super) fn inputs(html: &str) -> Result<Vec<Input>> {
    let html = Regex::new(r"(?s)<!--[\s\S]*?-->")?.replace_all(html, "");
    let sources = Regex::new(r"(?i)<source\b[^>]*>")?;
    let attr = Regex::new(r#"([\w-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)?;
    let pin = Regex::new(&format!(
        r"^{}/([A-Za-z0-9][A-Za-z0-9._-]*)/([a-z-]+\.mp4)$",
        regex::escape(&crate::repository::releases())
    ))?;
    let mut inputs = Vec::new();
    for tag in sources.find_iter(&html) {
        let mut attrs = BTreeMap::new();
        for capture in attr.captures_iter(tag.as_str()) {
            let key = capture[1].to_owned();
            let value = capture
                .get(2)
                .or_else(|| capture.get(3))
                .unwrap()
                .as_str()
                .to_owned();
            ensure!(
                attrs.insert(key.clone(), value).is_none(),
                "duplicate source attribute: {key}"
            );
        }
        let url = attrs
            .get("data-release-url")
            .context("video source needs an exact pinned release URL")?;
        let capture = pin
            .captures(url)
            .context("video source needs an exact pinned release URL")?;
        let release = capture[1].to_owned();
        let name = capture[2].to_owned();
        ensure!(
            !["latest", "main", "master"].contains(&release.as_str()),
            "video source needs an exact pinned release URL"
        );
        let src = format!("./media/{name}");
        ensure!(
            NAMES.contains(&name.as_str())
                && attrs.get("src") == Some(&src)
                && attrs.get("type").is_some_and(|v| v == "video/mp4"),
            "video source must use its matching local MP4 path"
        );
        inputs.push(Input {
            name,
            release,
            src,
            url: url.clone(),
        });
    }
    ensure!(
        inputs.len() == NAMES.len()
            && inputs
                .iter()
                .map(|s| &s.name)
                .collect::<BTreeSet<_>>()
                .len()
                == NAMES.len(),
        "showcase needs exactly one source for each flagship"
    );
    ensure!(
        inputs
            .iter()
            .map(|s| &s.release)
            .collect::<BTreeSet<_>>()
            .len()
            == 1,
        "showcase media must use one release"
    );
    Ok(inputs)
}

fn assets(
    manifest: &Value,
    inputs: &[Input],
    release: &Value,
    commit: &Value,
) -> Result<Vec<Asset>> {
    let tag = &inputs[0].release;
    ensure!(
        release["tag_name"] == *tag
            && release["draft"] == false
            && release["published_at"]
                .as_str()
                .is_some_and(|s| time::OffsetDateTime::parse(
                    s,
                    &time::format_description::well_known::Rfc3339
                )
                .is_ok()),
        "showcase release is not published"
    );
    ensure!(
        manifest["schema_version"] == 1 && manifest["release"] == *tag,
        "manifest schema or release differs from source pin"
    );
    let sha = manifest["source_commit"].as_str().unwrap_or_default();
    ensure!(
        Regex::new(r"^[a-f0-9]{40}$")?.is_match(sha) && commit["sha"] == sha,
        "manifest source commit differs from release tag"
    );
    let manifest_assets = manifest["assets"]
        .as_array()
        .context("missing manifest asset inventory")?;
    let published_assets = release["assets"]
        .as_array()
        .context("missing release asset inventory")?;
    let name_pattern = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")?;
    for inventory in [manifest_assets, published_assets] {
        ensure!(
            inventory.len() <= 1000
                && inventory
                    .iter()
                    .map(|a| a["name"].as_str())
                    .collect::<BTreeSet<_>>()
                    .len()
                    == inventory.len(),
            "duplicate or oversized asset inventory"
        );
        ensure!(
            inventory
                .iter()
                .all(|a| a["name"].as_str().is_some_and(|s| name_pattern.is_match(s))),
            "invalid asset name"
        );
    }
    inputs
        .iter()
        .map(|input| {
            let asset = manifest_assets
                .iter()
                .find(|a| a["name"] == input.name)
                .context("missing manifest media")?;
            let published = published_assets
                .iter()
                .find(|a| a["name"] == input.name)
                .context("missing published media")?;
            let bytes = asset["bytes"].as_u64().context("invalid media size")?;
            let sha256 = asset["sha256"].as_str().context("missing media checksum")?;
            ensure!(
                (12..=MAX_MEDIA).contains(&bytes)
                    && Regex::new(r"^[a-f0-9]{64}$")?.is_match(sha256),
                "invalid media metadata: {}",
                input.name
            );
            ensure!(
                published["size"].as_u64() == Some(bytes)
                    && published["browser_download_url"].as_str() == Some(input.url.as_str()),
                "published media differs from manifest: {}",
                input.name
            );
            ensure!(
                published["digest"].is_null()
                    || published["digest"] == ""
                    || published["digest"] == format!("sha256:{sha256}"),
                "published media digest differs: {}",
                input.name
            );
            Ok(Asset {
                input: input.clone(),
                bytes,
                sha256: sha256.into(),
            })
        })
        .collect()
}

fn read_json(network: &impl Network, url: &str) -> Result<Value> {
    let response = network.open(url)?;
    ensure!(
        response.length.is_none_or(|size| size <= MAX_JSON),
        "JSON download exceeds limit: {url}"
    );
    let mut bytes = Vec::new();
    response.reader.take(MAX_JSON + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_JSON,
        "JSON download exceeds limit: {url}"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

fn resolve_release(network: &impl Network, inputs: &[Input]) -> Result<(Value, Value, Vec<Asset>)> {
    let api = crate::repository::api();
    let base = crate::repository::releases();
    let tag = &inputs[0].release;
    let release = read_json(network, &format!("{api}/releases/tags/{tag}"))?;
    ensure!(
        release["tag_name"] == *tag && release["draft"] == false,
        "showcase release is not public"
    );
    let manifest_url = format!("{base}/{tag}/release-manifest.json");
    ensure!(
        release["assets"].as_array().is_some_and(|assets| assets
            .iter()
            .filter(|a| a["name"] == "release-manifest.json"
                && a["browser_download_url"].as_str() == Some(manifest_url.as_str()))
            .count()
            == 1),
        "missing public release manifest"
    );
    let manifest = read_json(network, &manifest_url)?;
    let commit = read_json(network, &format!("{api}/commits/{tag}"))?;
    let assets = assets(&manifest, inputs, &release, &commit)?;
    Ok((release, manifest, assets))
}

fn identity(release: &Value) -> Value {
    let names = NAMES.into_iter().chain(["release-manifest.json"]);
    json!({"id":release["id"], "assets":names.map(|name| {
        let asset = release["assets"].as_array().and_then(|list| list.iter().find(|a| a["name"] == name));
        asset.map(|a| json!([a["name"], a["id"], a["size"], a["updated_at"], a["digest"], a["browser_download_url"]]))
    }).collect::<Vec<_>>()})
}

fn download(network: &impl Network, asset: &Asset, path: &Path) -> Result<()> {
    let mut response = network.open(&asset.input.url)?;
    ensure!(
        response.length.is_none_or(|size| size == asset.bytes),
        "content length differs: {}",
        asset.input.name
    );
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut prefix = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = response.reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        ensure!(
            bytes <= asset.bytes,
            "media exceeds declared size: {}",
            asset.input.name
        );
        prefix.extend(
            buffer[..count]
                .iter()
                .take(12usize.saturating_sub(prefix.len())),
        );
        hash.update(&buffer[..count]);
        file.write_all(&buffer[..count])?;
    }
    ensure!(
        bytes == asset.bytes && crate::hash::hex(&hash.finalize()) == asset.sha256,
        "media hash or size differs: {}",
        asset.input.name
    );
    ensure!(
        prefix.get(4..8) == Some(b"ftyp".as_slice()),
        "media is not an MP4: {}",
        asset.input.name
    );
    file.sync_all()?;
    Ok(())
}

fn stage(network: &impl Network, html: &str, site: &Path) -> Result<()> {
    let api = crate::repository::api();
    let inputs = inputs(html)?;
    let destination = site.join("media");
    fs::create_dir(&destination).context("reserve fresh showcase media directory")?;
    let result = (|| -> Result<()> {
        let (release, manifest, assets) = resolve_release(network, &inputs)?;
        let staging = tempfile::tempdir_in(site)?;
        for asset in &assets {
            download(network, asset, &staging.path().join(&asset.input.name))?;
        }
        let latest_release = read_json(
            network,
            &format!("{api}/releases/tags/{}", inputs[0].release),
        )?;
        let latest_commit = read_json(network, &format!("{api}/commits/{}", inputs[0].release))?;
        self::assets(&manifest, &inputs, &latest_release, &latest_commit)?;
        ensure!(
            identity(&release) == identity(&latest_release),
            "release assets changed during download; retry after publication finishes"
        );
        fs::remove_dir(&destination)?;
        fs::rename(staging.path(), &destination)?;
        Ok(())
    })();
    if result.is_err() && destination.exists() {
        fs::remove_dir(&destination).context("remove empty media reservation")?;
    }
    result
}

pub(super) fn run(root: &Path, verify: bool) -> Result<()> {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    let network = Http {
        agent: config.into(),
        api_token: std::env::var("GH_TOKEN")
            .or_else(|_| std::env::var("GITHUB_TOKEN"))
            .ok()
            .filter(|token| !token.trim().is_empty()),
    };
    let html = fs::read_to_string(root.join("knowledge/showcase.html"))?;
    if verify {
        resolve_release(&network, &inputs(&html)?)?;
    } else {
        stage(&network, &html, &root.join("_site"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, io::Cursor};

    #[test]
    fn api_authentication_is_confined_and_public_downloads_stay_anonymous() {
        let network = Http {
            agent: ureq::Agent::new_with_defaults(),
            api_token: Some("fixture-token".into()),
        };
        assert!(network
            .token_for(&format!(
                "{}/releases/tags/preview",
                crate::repository::api()
            ))
            .is_some());
        for url in [
            format!(
                "{}/preview/release-manifest.json",
                crate::repository::releases()
            ),
            format!("{}.attacker.example/releases", crate::repository::api()),
            "https://api.github.com.attacker.example/repos/x/y".into(),
            "https://api.github.com/repos/someone/else/releases".into(),
            format!(
                "http://api.github.com/repos/{}/releases",
                crate::repository::slug()
            ),
        ] {
            assert!(network.token_for(&url).is_none());
        }
    }
    struct Fixture {
        requests: RefCell<Vec<String>>,
        release: Value,
        manifest: Value,
        media: Vec<u8>,
        corrupt: bool,
        moved: bool,
        replaced: bool,
    }
    impl Fixture {
        fn new() -> Self {
            let base = crate::repository::releases();
            let media = b"\0\0\0\x18ftypisom00000000".to_vec();
            let digest = crate::hash::hex(&Sha256::digest(&media));
            let assets: Vec<_> = NAMES
                .map(|name| json!({"name":name,"bytes":media.len(),"sha256":digest}))
                .into();
            let manifest = json!({"schema_version":1,"release":"preview-test","source_commit":"a".repeat(40),"assets":assets});
            let mut assets: Vec<_> = NAMES.map(|name| json!({"name":name,"size":media.len(),"digest":format!("sha256:{digest}"),"browser_download_url":format!("{base}/preview-test/{name}")})).into();
            assets.push(json!({"name":"release-manifest.json","browser_download_url":format!("{base}/preview-test/release-manifest.json")}));
            Self {
                requests: RefCell::new(Vec::new()),
                release: json!({"tag_name":"preview-test","draft":false,"published_at":"2026-09-12T00:00:00Z","assets":assets}),
                manifest,
                media,
                corrupt: false,
                moved: false,
                replaced: false,
            }
        }
        fn html(&self) -> String {
            let base = crate::repository::releases();
            NAMES.map(|name| format!(r#"<source src="./media/{name}" data-release-url="{base}/preview-test/{name}" type="video/mp4">"#)).join("\n")
        }
    }
    impl Network for Fixture {
        fn open(&self, url: &str) -> Result<Download> {
            self.requests.borrow_mut().push(url.into());
            let bytes = if url.contains("/releases/tags/") {
                let mut release = self.release.clone();
                if self.replaced && self.requests.borrow().len() > 3 {
                    release["assets"][3]["id"] = json!("replacement");
                }
                serde_json::to_vec(&release)?
            } else if url.ends_with("release-manifest.json") {
                serde_json::to_vec(&self.manifest)?
            } else if url.contains("/commits/") {
                serde_json::to_vec(
                    &json!({"sha":if self.moved && self.requests.borrow().len() > 3 {"b".repeat(40)} else {"a".repeat(40)}}),
                )?
            } else {
                ensure!(
                    NAMES.iter().any(|name| url.ends_with(name)),
                    "unexpected fixture URL"
                );
                if self.corrupt {
                    vec![0; self.media.len()]
                } else {
                    self.media.clone()
                }
            };
            Ok(Download {
                length: Some(bytes.len() as u64),
                reader: Box::new(Cursor::new(bytes)),
            })
        }
    }
    #[test]
    fn published_release_verifies_and_stages_pinned_videos() {
        let network = Fixture::new();
        resolve_release(&network, &inputs(&network.html()).unwrap()).unwrap();
        let site = tempfile::tempdir().unwrap();
        stage(&network, &network.html(), site.path()).unwrap();
        for name in NAMES {
            assert_eq!(
                fs::read(site.path().join("media").join(name)).unwrap(),
                network.media
            );
        }
    }

    #[test]
    fn published_media_urls_require_the_exact_repository_tag_and_file() {
        let network = Fixture::new();
        let input = inputs(&network.html()).unwrap();
        for url in [
            "https://github.com/other/Limo-CAD/releases/download/preview-test/bench-build-full.mp4",
            "https://github.com/limo-cad/Limo-CAD-fork/releases/download/preview-test/bench-build-full.mp4",
            "https://github.com/limo-cad/Limo-CAD/releases/download/other-tag/bench-build-full.mp4",
            "https://github.com/limo-cad/Limo-CAD/releases/download/preview-test/other.mp4",
        ] {
            let mut release = network.release.clone();
            release["assets"][0]["browser_download_url"] = json!(url);
            assert!(assets(&network.manifest, &input, &release, &json!({"sha":"a".repeat(40)})).is_err());
        }
    }
    #[test]
    fn pins_and_inventory_reject_ambiguous_unpublished_or_corrupt_inputs() {
        let fixture = Fixture::new();
        let html = fixture.html();
        let input = inputs(&html).unwrap();
        for broken in [
            format!("<!-- {html} -->"),
            html.replace("preview-test", "latest"),
            html.clone() + &html,
            html.replace("src=", "src=\"oops\" src="),
            html.replacen("preview-test", "another-tag", 1),
        ] {
            assert!(inputs(&broken).is_err());
        }
        assets(
            &fixture.manifest,
            &input,
            &fixture.release,
            &json!({"sha":"a".repeat(40)}),
        )
        .unwrap();
        for bad in [json!(null), json!(1.5), json!(MAX_MEDIA + 1)] {
            let mut manifest = fixture.manifest.clone();
            manifest["assets"][0]["bytes"] = bad;
            assert!(assets(
                &manifest,
                &input,
                &fixture.release,
                &json!({"sha":"a".repeat(40)})
            )
            .is_err());
        }
        let mut release = fixture.release.clone();
        release["draft"] = json!(true);
        assert!(assets(
            &fixture.manifest,
            &input,
            &release,
            &json!({"sha":"a".repeat(40)})
        )
        .is_err());
    }
    #[test]
    fn staging_is_atomic_and_stale_output_stays_untouched() {
        let site = tempfile::tempdir().unwrap();
        let network = Fixture::new();
        stage(&network, &network.html(), site.path()).unwrap();
        assert_eq!(network.requests.borrow().len(), 8);
        for name in NAMES {
            assert_eq!(
                fs::read(site.path().join("media").join(name)).unwrap(),
                network.media
            );
        }
        let network = Fixture::new();
        assert!(stage(&network, &network.html(), site.path()).is_err());
        assert!(network.requests.borrow().is_empty());
    }
    #[test]
    fn failed_download_or_changed_release_leaves_no_publishable_media() {
        for mode in ["corrupt", "moved", "replaced", "draft"] {
            let mut network = Fixture::new();
            network.corrupt = mode == "corrupt";
            network.moved = mode == "moved";
            network.replaced = mode == "replaced";
            if mode == "draft" {
                network.release["draft"] = json!(true);
            }
            let site = tempfile::tempdir().unwrap();
            assert!(stage(&network, &network.html(), site.path()).is_err());
            assert_eq!(fs::read_dir(site.path()).unwrap().count(), 0);
        }
        let network = Fixture::new();
        resolve_release(&network, &inputs(&network.html()).unwrap()).unwrap();
        assert_eq!(network.requests.borrow().len(), 3);
    }
}
