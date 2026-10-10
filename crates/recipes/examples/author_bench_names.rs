//! Refresh bench history labels while preserving every existing construction
//! block and manufacturing input. Run from the repository root with
//! `cargo run -p limo-cad-recipes --example author_bench_names`.
use limo_cad_recipes::authoring::feature_name_step;
use serde_json::Value;

fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/scripts/garden-bench.limo.jsonc");
    let source = std::fs::read_to_string(&path).unwrap();
    let document: Value = serde_json::from_str(&source).unwrap();
    let steps = document["steps"].as_array().unwrap();
    let start = source.find("  \"steps\": [").unwrap() + "  \"steps\": [".len();
    let end = start + source[start..].find("\n  ]").unwrap();
    let mut cursor = start;
    let mut blocks = Vec::new();
    while let Some(next) = source[cursor..end].find("\n    {") {
        let begin = cursor + next + 1;
        let mut values =
            serde_json::Deserializer::from_str(&source[begin..end]).into_iter::<Value>();
        let value = values.next().unwrap().unwrap();
        let close = begin + values.byte_offset();
        let block = &source[begin..close];
        assert_eq!(value, steps[blocks.len()]);
        blocks.push(block);
        cursor = close;
    }
    assert_eq!(blocks.len(), steps.len());
    let mut output = Vec::new();
    let mut index = 0;
    while index < steps.len() {
        let step = &steps[index];
        output.push(blocks[index].to_owned());
        let naming = step["id"].as_str().and_then(|id| {
            feature_name_step(
                id,
                step["call"]["operation"].as_str().unwrap_or(""),
                &step["call"]["arguments"],
            )
        });
        if let Some(naming) = naming {
            if steps
                .get(index + 1)
                .is_some_and(|next| next["id"] == naming["id"])
            {
                index += 1;
            }
            output.push(
                serde_json::to_string_pretty(&naming)
                    .unwrap()
                    .lines()
                    .map(|line| format!("    {line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        index += 1;
    }
    let output = format!(
        "{}\n{}{}",
        &source[..start],
        output.join(",\n"),
        &source[end..]
    );
    limo_cad_script::Script::parse(&output).expect("Named bench source preflight");
    std::fs::write(path, output).unwrap();
}
