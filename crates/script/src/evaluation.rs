//! Allocation/work accounting for the shared interpreter, never another evaluator.
use crate::RunProgress;
use serde_json::Value;

/// Opt-in, cumulative limits for one evaluation (including intermediate copies).
/// No credit is returned when a temporary or binding is dropped. `max_bytes`
/// counts UTF-8 string/key bytes plus 128 bytes per Value and per object/binding
/// entry, conservatively allowing for container capacity and bookkeeping. It is
/// a data allocation budget, not a process RSS limit. The host must bound its own
/// allocations before returning a result; results are checked before retention
/// or interpreter copies. Parsing source and synchronous host work are separate.
#[derive(Clone, Copy, Debug)]
pub struct EvaluationLimits {
    pub max_bytes: usize,
    pub max_nodes: usize,
    /// Maximum expression/value/predicate nesting; must be in 1..=128.
    pub max_depth: usize,
    /// Node visits, predicate/comparison work and 4 KiB text chunks.
    pub max_work: usize,
}

pub(crate) trait ExecutionHost {
    fn checkpoint(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn call(&mut self, name: &str, args: Value, progress: RunProgress) -> Result<Value, String>;
}

pub(crate) struct CallbackHost<F, C>(pub F, pub C);
impl<F, C> ExecutionHost for CallbackHost<F, C>
where
    F: FnMut(&str, Value, RunProgress) -> Result<Value, String>,
    C: FnMut() -> Result<(), String>,
{
    fn checkpoint(&mut self) -> Result<(), String> {
        (self.1)()
    }
    fn call(&mut self, name: &str, args: Value, progress: RunProgress) -> Result<Value, String> {
        (self.0)(name, args, progress)
    }
}

pub(crate) struct Evaluation<'a> {
    pub host: &'a mut dyn ExecutionHost,
    limits: Option<EvaluationLimits>,
    bytes: usize,
    nodes: usize,
    work: usize,
}
impl<'a> Evaluation<'a> {
    pub fn new(
        host: &'a mut dyn ExecutionHost,
        limits: Option<EvaluationLimits>,
    ) -> Result<Self, String> {
        if limits.is_some_and(|limits| !(1..=128).contains(&limits.max_depth)) {
            return Err("Evaluation depth limit must be in 1..=128".into());
        }
        Ok(Self {
            host,
            limits,
            bytes: 0,
            nodes: 0,
            work: 0,
        })
    }
    pub fn bounded(&self) -> bool {
        self.limits.is_some()
    }
    pub fn checkpoint(&mut self) -> Result<(), String> {
        if self.bounded() {
            self.host.checkpoint()?;
        }
        Ok(())
    }
    pub fn visit(&mut self, depth: usize) -> Result<(), String> {
        let Some(limits) = self.limits else {
            return Ok(());
        };
        self.checkpoint()?;
        if depth > limits.max_depth {
            return Err("Script evaluation depth limit exceeded".into());
        }
        self.work = self
            .work
            .checked_add(1)
            .filter(|n| *n <= limits.max_work)
            .ok_or("Script evaluation work limit exceeded")?;
        Ok(())
    }
    pub fn charge(&mut self, nodes: usize, bytes: usize) -> Result<(), String> {
        let Some(limits) = self.limits else {
            return Ok(());
        };
        let nodes = self
            .nodes
            .checked_add(nodes)
            .filter(|n| *n <= limits.max_nodes)
            .ok_or("Script evaluation node limit exceeded")?;
        let bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= limits.max_bytes)
            .ok_or("Script evaluation byte limit exceeded")?;
        self.nodes = nodes;
        self.bytes = bytes;
        Ok(())
    }
    pub fn text_work(&mut self, len: usize) -> Result<(), String> {
        if self.bounded() {
            for _ in 0..len.div_ceil(4096) {
                self.visit(0)?;
            }
        }
        Ok(())
    }
    /// Reserve a key and map entry before cloning/inserting it.
    pub fn key(&mut self, key: &str) -> Result<(), String> {
        if self.bounded() {
            self.charge(
                0,
                key.len()
                    .checked_add(128)
                    .ok_or("Script evaluation byte limit exceeded")?,
            )?;
            self.text_work(key.len())?;
        }
        Ok(())
    }
    pub fn node(&mut self, value: &Value, depth: usize) -> Result<(), String> {
        self.visit(depth)?;
        if self.bounded() {
            let len = value.as_str().map_or(0, str::len);
            self.charge(
                1,
                len.checked_add(128)
                    .ok_or("Script evaluation byte limit exceeded")?,
            )?;
            self.text_work(len)?;
        }
        Ok(())
    }
    /// Account an existing source/host value with early termination. Never
    /// serialize, clone, or allocate a traversal stack just to measure it.
    pub fn account(&mut self, value: &Value, depth: usize) -> Result<(), String> {
        if !self.bounded() {
            return Ok(());
        }
        self.node(value, depth)?;
        match value {
            Value::Array(values) => {
                for value in values {
                    self.account(value, depth + 1)?;
                }
            }
            Value::Object(values) => {
                for (key, value) in values {
                    self.key(key)?;
                    self.account(value, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// Copies one node/key at a time, charging before each allocation. Arrays
    /// grow only after their next element has been charged; no untrusted length
    /// is passed to with_capacity/collect ahead of the budget checks.
    pub fn cloned(&mut self, value: &Value, depth: usize) -> Result<Value, String> {
        if !self.bounded() {
            return Ok(value.clone());
        }
        self.node(value, depth)?;
        Ok(match value {
            Value::Array(values) => {
                let mut result = Vec::new();
                for value in values {
                    result.push(self.cloned(value, depth + 1)?);
                }
                Value::Array(result)
            }
            Value::Object(values) => {
                let mut result = serde_json::Map::new();
                for (key, value) in values {
                    self.key(key)?;
                    let value = self.cloned(value, depth + 1)?;
                    result.insert(key.clone(), value);
                }
                Value::Object(result)
            }
            _ => value.clone(),
        })
    }
    pub fn pointer<'v>(&mut self, value: &'v Value, path: &str) -> Result<&'v Value, String> {
        self.lookup(value, path)?.ok_or_else(|| {
            self.diagnostic("Missing result path", || {
                format!("Missing result path {path}")
            })
        })
    }
    pub fn lookup<'v>(
        &mut self,
        value: &'v Value,
        path: &str,
    ) -> Result<Option<&'v Value>, String> {
        if self.bounded() {
            // serde_json decodes pointer tokens into temporary strings. Reserve
            // both replacements before entering it, and bound token depth/work.
            self.charge(
                0,
                path.len()
                    .checked_mul(2)
                    .ok_or("Script evaluation byte limit exceeded")?,
            )?;
            self.text_work(path.len())?;
            for (depth, _) in path.split('/').enumerate() {
                self.visit(depth)?;
            }
        }
        Ok(if path.is_empty() {
            Some(value)
        } else {
            value.pointer(path)
        })
    }
    /// Text-returning legacy hosts remain supported, with a pessimistic bound
    /// paid BEFORE JSON decoding: at most one node and 256 bytes per input byte.
    /// This also bounds malformed JSON and parser scratch. Structured host
    /// results avoid this reservation. The normal trusted path is unchanged.
    pub fn response(&mut self, mut value: Value) -> Result<Value, String> {
        self.checkpoint()?;
        self.account(&value, 0)?;
        if let Some(text) = value.as_str() {
            if let Some(limits) = self.limits {
                let units = text
                    .len()
                    .checked_add(1)
                    .ok_or("Script evaluation node limit exceeded")?;
                self.charge(
                    units,
                    units
                        .checked_mul(256)
                        .ok_or("Script evaluation byte limit exceeded")?,
                )?;
                let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
                for (index, byte) in text.bytes().enumerate() {
                    if index % 4096 == 0 {
                        self.visit(0)?;
                    }
                    if quoted {
                        if escaped {
                            escaped = false;
                        } else if byte == b'\\' {
                            escaped = true;
                        } else if byte == b'"' {
                            quoted = false;
                        }
                    } else if byte == b'"' {
                        quoted = true;
                    } else if matches!(byte, b'[' | b'{') {
                        depth += 1;
                        if depth > limits.max_depth {
                            return Err("Script evaluation depth limit exceeded".into());
                        }
                    } else if matches!(byte, b']' | b'}') {
                        depth = depth.saturating_sub(1);
                    }
                }
            }
            if let Ok(parsed) = serde_json::from_str(text) {
                value = parsed;
            }
        }
        self.checkpoint()?;
        Ok(value)
    }
    pub fn call(
        &mut self,
        name: &str,
        args: Value,
        progress: RunProgress,
        decode_text: bool,
    ) -> Result<Value, String> {
        self.checkpoint()?;
        let result = self.host.call(name, args, progress)?;
        if decode_text {
            self.response(result)
        } else {
            self.checkpoint()?;
            self.account(&result, 0)?;
            Ok(result)
        }
    }
    pub fn diagnostic(&self, summary: &str, detail: impl FnOnce() -> String) -> String {
        if self.bounded() {
            summary.to_owned()
        } else {
            detail()
        }
    }
}
