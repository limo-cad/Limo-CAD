//! Bounded observations of the launched host and its current descendants.
//! These are cumulative process counters, not total application resource use:
//! exited children are absent, and summed RSS counts shared pages repeatedly.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Instant,
};

const MAX_SCANNED: usize = 4096;
const MAX_OWNED: usize = 256;
const MAX_FILE_BYTES: u64 = 16_384;
const IO_KEYS: [&str; 7] = [
    "rchar",
    "wchar",
    "syscr",
    "syscw",
    "read_bytes",
    "write_bytes",
    "cancelled_write_bytes",
];

#[derive(Clone, Debug, PartialEq)]
struct Stat {
    pid: u32,
    parent: u32,
    start_ticks: u64,
    user_ticks: u64,
    system_ticks: u64,
    rss_pages: u64,
    name: String,
}
fn stat(text: &str) -> Option<Stat> {
    let (prefix, rest) = text.split_once('(')?;
    let (name, fields) = rest.rsplit_once(')')?;
    let fields: Vec<_> = fields.split_whitespace().collect();
    let value = |n: usize| fields.get(n)?.parse::<u64>().ok();
    Some(Stat {
        pid: prefix.trim().parse().ok()?,
        parent: u32::try_from(value(1)?).ok()?,
        start_ticks: value(19)?,
        user_ticks: value(11)?,
        system_ticks: value(12)?,
        rss_pages: value(21)?,
        name: name.into(),
    })
}
fn read(path: &Path) -> Result<String, String> {
    let mut text = String::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err("proc file exceeds observation limit".into());
    }
    Ok(text)
}
fn read_stat(root: &Path, pid: u32) -> Result<Stat, String> {
    stat(&read(&root.join(pid.to_string()).join("stat"))?)
        .filter(|s| s.pid == pid)
        .ok_or_else(|| "malformed process stat".into())
}
fn descendants(root: u32, all: &BTreeMap<u32, Stat>) -> (BTreeSet<u32>, bool) {
    let mut owned = BTreeSet::from([root]);
    loop {
        let mut changed = false;
        for (&pid, item) in all {
            if !owned.contains(&pid) && owned.contains(&item.parent) {
                let Some(parent) = all.get(&item.parent) else {
                    continue;
                };
                if item.start_ticks < parent.start_ticks {
                    continue;
                }
                if owned.len() == MAX_OWNED {
                    return (owned, true);
                }
                owned.insert(pid);
                changed = true;
            }
        }
        if !changed {
            return (owned, false);
        }
    }
}
fn io(text: &str) -> Option<BTreeMap<String, u64>> {
    let values: BTreeMap<_, _> = text
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_owned(), value.trim().parse::<u64>().ok()?))
        })
        .collect();
    IO_KEYS
        .iter()
        .all(|key| values.contains_key(*key))
        .then_some(values)
}

pub(super) fn sample(pid: u32) -> Value {
    observe(Path::new("/proc"), pid)
}
fn observe(root: &Path, pid: u32) -> Value {
    let start = Instant::now();
    let mut issues = Vec::new();
    let mut all = BTreeMap::new();
    let initial = match read_stat(root, pid) {
        Ok(value) => value,
        Err(error) => return json!({"root_pid":pid,"complete":false,"error":error}),
    };
    all.insert(pid, initial.clone());
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => return json!({"root_pid":pid,"complete":false,"error":error.to_string()}),
    };
    let mut scanned = 0;
    let mut truncated = false;
    for entry in entries {
        let Ok(entry) = entry else {
            issues.push(json!({"phase":"enumerate","error":"unreadable proc entry"}));
            continue;
        };
        let Some(child) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if scanned == MAX_SCANNED {
            truncated = true;
            break;
        }
        scanned += 1;
        if child == pid {
            continue;
        }
        match read_stat(root, child) {
            Ok(value) => {
                all.insert(child, value);
            }
            Err(error) => issues.push(json!({"pid":child,"phase":"scan","error":error})),
        }
    }
    let (owned, owned_truncated) = descendants(pid, &all);
    let mut processes = Vec::new();
    let mut user_ticks = 0u64;
    let mut system_ticks = 0u64;
    let mut rss_pages = 0u64;
    let mut io_totals = BTreeMap::<String, u64>::new();
    let mut io_count = 0;
    for child in owned {
        let Some(before) = all.get(&child) else {
            continue;
        };
        let directory: PathBuf = root.join(child.to_string());
        let io = read(&directory.join("io")).ok().and_then(|text| io(&text));
        let after = match read_stat(root, child) {
            Ok(after)
                if before.start_ticks == after.start_ticks && before.parent == after.parent =>
            {
                after
            }
            _ => {
                issues.push(json!({"pid":child,"phase":"observe","error":"process exited, changed owner or PID identity"}));
                continue;
            }
        };
        if io.is_none() {
            issues.push(
                json!({"pid":child,"phase":"io","error":"unavailable or malformed I/O counters"}),
            );
        }
        for (sum, value) in [
            (&mut user_ticks, after.user_ticks),
            (&mut system_ticks, after.system_ticks),
            (&mut rss_pages, after.rss_pages),
        ] {
            if sum.checked_add(value).is_none() {
                issues.push(json!({"phase":"sum","error":"counter overflow"}));
            }
            *sum = sum.saturating_add(value);
        }
        if let Some(io) = &io {
            io_count += 1;
            for key in IO_KEYS {
                let sum = io_totals.entry(key.into()).or_default();
                if sum.checked_add(io[key]).is_none() {
                    issues.push(json!({"phase":"sum","error":"I/O counter overflow"}));
                }
                *sum = sum.saturating_add(io[key]);
            }
        }
        processes.push(json!({"pid":child,"parent_pid":after.parent,"start_ticks":after.start_ticks,"name":after.name,
            "user_ticks":after.user_ticks,"system_ticks":after.system_ticks,"rss_pages":after.rss_pages,"io":io}));
    }
    let root_still_matches =
        read_stat(root, pid).is_ok_and(|now| now.start_ticks == initial.start_ticks);
    if !root_still_matches {
        issues.push(json!({"phase":"root","error":"launched root exited or PID identity changed"}));
    }
    json!({"root_pid":pid,"root_start_ticks":initial.start_ticks,"complete":issues.is_empty() && !truncated && !owned_truncated,
        "scanned_processes":scanned,"scan_limit":MAX_SCANNED,"owned_limit":MAX_OWNED,"truncated":truncated || owned_truncated,
        "observed_processes":processes.len(),"processes":processes,"issues":issues,
        "totals":{"user_ticks":user_ticks,"system_ticks":system_ticks,"rss_pages_sum":rss_pages,"io":io_totals,"io_processes_observed":io_count},
        "observation_ms":start.elapsed().as_secs_f64()*1000.,
        "limitations":["not an atomic snapshot","exited or reparented children and between-sample work may be absent",
            "RSS sum may count shared pages repeatedly; not physical or GPU memory","CPU ticks and I/O are cumulative per live PID/start_ticks identity"]})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(pid: u32, parent: u32, start: u64) -> Stat {
        Stat {
            pid,
            parent,
            start_ticks: start,
            user_ticks: 11,
            system_ticks: 7,
            rss_pages: 5,
            name: "test".into(),
        }
    }
    fn stat_text(item: &Stat) -> String {
        let mut fields = vec!["0".to_owned(); 22];
        fields[0] = "S".into();
        for (index, value) in [
            (1, u64::from(item.parent)),
            (11, item.user_ticks),
            (12, item.system_ticks),
            (19, item.start_ticks),
            (21, item.rss_pages),
        ] {
            fields[index] = value.to_string();
        }
        format!("{} ({}) {}", item.pid, item.name, fields.join(" "))
    }
    #[test]
    fn process_identity_parses_names_and_counter_positions() {
        let mut fields = vec!["0"; 22];
        fields[0] = "S";
        for (index, value) in [(1, "4"), (11, "11"), (12, "7"), (19, "30"), (21, "5")] {
            fields[index] = value;
        }
        let parsed = stat(&format!("8 (CAD (owned) worker) {}", fields.join(" "))).unwrap();
        assert_eq!(
            parsed,
            Stat {
                name: "CAD (owned) worker".into(),
                ..record(8, 4, 30)
            }
        );
        assert!(stat("8 (bad) S 4").is_none());
    }
    #[test]
    fn ownership_includes_nested_children_and_excludes_other_instances() {
        let all = [
            (40, 1, 10),
            (8, 40, 20),
            (9, 8, 25),
            (50, 1, 10),
            (51, 50, 20),
            (7, 40, 5),
        ]
        .map(|(pid, parent, start)| (pid, record(pid, parent, start)))
        .into_iter()
        .collect();
        assert_eq!(descendants(40, &all), (BTreeSet::from([8, 9, 40]), false));
        let wide = (2..300)
            .map(|pid| (pid, record(pid, 1, 20)))
            .chain([(1, record(1, 0, 10))])
            .collect();
        let (owned, truncated) = descendants(1, &wide);
        assert_eq!(owned.len(), MAX_OWNED);
        assert!(truncated);
    }
    #[test]
    fn io_requires_all_kernel_counters_without_turning_missing_values_into_zero() {
        assert!(io("read_bytes: 5\n").is_none());
        let text = IO_KEYS
            .iter()
            .enumerate()
            .map(|(n, key)| format!("{key}: {n}\n"))
            .collect::<String>();
        assert_eq!(io(&text).unwrap()["write_bytes"], 5);
    }
    #[test]
    fn process_tree_totals_include_owned_children_but_not_another_cad_instance() {
        struct Directory(PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = Directory(std::env::temp_dir().join(format!(
            "limo-cad-proc-sample-{}-{nonce}",
            std::process::id()
        )));
        fs::create_dir(&root.0).unwrap();
        let io_text = IO_KEYS
            .iter()
            .map(|key| format!("{key}: 2\n"))
            .collect::<String>();
        for item in [
            record(40, 1, 10),
            record(8, 40, 20),
            record(9, 8, 25),
            record(50, 1, 10),
            record(51, 50, 20),
        ] {
            let directory = root.0.join(item.pid.to_string());
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("stat"), stat_text(&item)).unwrap();
            fs::write(directory.join("io"), &io_text).unwrap();
        }
        let sample = observe(&root.0, 40);
        assert_eq!(sample["complete"], true);
        assert_eq!(sample["observed_processes"], 3);
        assert_eq!(sample["totals"]["user_ticks"], 33);
        assert_eq!(sample["totals"]["system_ticks"], 21);
        assert_eq!(sample["totals"]["rss_pages_sum"], 15);
        assert_eq!(sample["totals"]["io"]["read_bytes"], 6);
        fs::write(root.0.join("8/io"), "unavailable").unwrap();
        let partial = observe(&root.0, 40);
        assert_eq!(partial["complete"], false);
        assert_eq!(partial["totals"]["io_processes_observed"], 2);
        assert_eq!(partial["totals"]["io"]["read_bytes"], 4);
    }
}
