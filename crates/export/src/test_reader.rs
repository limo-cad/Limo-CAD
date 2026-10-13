//! Independent XML reader used by manufacturing acceptance tests.
//! Expands the actual build graph and 3MF matrices without assembly math.
use std::collections::HashMap;
use std::io::Read;

pub fn read_package_text(bytes: &[u8], path: &str) -> Result<String, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut text = String::new();
    zip.by_name(path)
        .map_err(|e| e.to_string())?
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

pub fn read_package_object_ids(bytes: &[u8], path: &str) -> Result<Vec<String>, String> {
    let text = read_package_text(bytes, path)?;
    let document = roxmltree::Document::parse(&text).map_err(|e| e.to_string())?;
    document
        .descendants()
        .filter(|n| n.has_tag_name("object"))
        .map(|n| {
            n.attribute("id")
                .map(str::to_owned)
                .ok_or_else(|| "Missing object id".into())
        })
        .collect()
}

/// Normalize Production Extension part paths into unique IDs, then use the
/// same independent matrix reader for both portable and slicer project files.
pub fn read_package(bytes: &[u8]) -> Result<Vec<ModelMesh>, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut resources = String::new();
    let mut build = String::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        if !file.name().ends_with(".model") {
            continue;
        }
        let path = format!("/{}", file.name());
        let mut text = String::new();
        file.read_to_string(&mut text).map_err(|e| e.to_string())?;
        let doc = roxmltree::Document::parse(&text).map_err(|e| e.to_string())?;
        for node in doc
            .descendants()
            .filter(|n| n.has_tag_name("object") || n.has_tag_name("item"))
        {
            let mut fragment = text[node.range()].to_string();
            let mut replacements = Vec::new();
            for reference in node.descendants().filter(|n| n.is_element()) {
                if let Some(id) = reference.attribute("objectid") {
                    let target_path = reference
                        .attribute((
                            "http://schemas.microsoft.com/3dmanufacturing/production/2015/06",
                            "path",
                        ))
                        .unwrap_or(&path);
                    let source = text[reference.range()].to_string();
                    let replaced = source.replacen(
                        &format!(" objectid=\"{id}\""),
                        &format!(
                            " objectid=\"{}#{id}\"",
                            crate::threemf::xml_escape(target_path)
                        ),
                        1,
                    );
                    replacements.push((source, replaced));
                }
            }
            for (from, to) in replacements {
                fragment = fragment.replace(&from, &to);
            }
            if let Some(id) = node.attribute("id") {
                fragment = fragment.replacen(
                    &format!(" id=\"{id}\""),
                    &format!(" id=\"{}#{id}\"", crate::threemf::xml_escape(&path)),
                    1,
                );
                resources.push_str(&fragment);
            } else if path == "/3D/3dmodel.model" {
                build.push_str(&fragment);
            }
        }
    }
    let xml = format!("<model xmlns:p=\"http://schemas.microsoft.com/3dmanufacturing/production/2015/06\"><resources>{resources}</resources><build>{build}</build></model>");
    read_build(&xml)
}

#[derive(Debug)]
pub struct ModelMesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[usize; 3]>,
    pub build_item: usize,
}

pub fn read_build(xml: &str) -> Result<Vec<ModelMesh>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let objects: HashMap<_, _> = doc
        .descendants()
        .filter(|n| n.has_tag_name("object"))
        .map(|n| (n.attribute("id").unwrap_or(""), n))
        .collect();
    fn numbers(text: &str) -> Result<Vec<f64>, String> {
        text.split_whitespace()
            .map(|v| v.parse().map_err(|_| "Invalid matrix".into()))
            .collect()
    }
    fn matrix(node: roxmltree::Node<'_, '_>) -> Result<[f64; 12], String> {
        let identity = [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.];
        let Some(text) = node.attribute("transform") else {
            return Ok(identity);
        };
        numbers(text)?
            .try_into()
            .map_err(|_| "Matrix must have twelve entries".into())
    }
    fn apply(m: &[f64; 12], v: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| v[0] * m[i] + v[1] * m[3 + i] + v[2] * m[6 + i] + m[9 + i])
    }
    fn expand<'a>(
        node: roxmltree::Node<'a, 'a>,
        objects: &HashMap<&'a str, roxmltree::Node<'a, 'a>>,
        transforms: &mut Vec<[f64; 12]>,
        visiting: &mut Vec<String>,
        output: &mut Vec<ModelMesh>,
    ) -> Result<(), String> {
        let id = node
            .attribute("objectid")
            .ok_or("Missing object reference")?;
        if visiting.iter().any(|v| v == id) {
            return Err("Cyclic object graph".into());
        }
        let object = objects.get(id).ok_or("Unknown object reference")?;
        visiting.push(id.into());
        transforms.push(matrix(node)?);
        if let Some(mesh) = object.children().find(|n| n.has_tag_name("mesh")) {
            let mut vertices = Vec::new();
            for vertex in mesh.descendants().filter(|n| n.has_tag_name("vertex")) {
                let mut v = [0.; 3];
                for (i, axis) in ["x", "y", "z"].iter().enumerate() {
                    v[i] = vertex
                        .attribute(*axis)
                        .ok_or("Missing coordinate")?
                        .parse()
                        .map_err(|_| "Invalid coordinate")?;
                }
                for transform in transforms.iter().rev() {
                    v = apply(transform, v);
                }
                if v.iter().any(|v| !v.is_finite()) {
                    return Err("Non-finite transformed vertex".into());
                }
                vertices.push(v);
            }
            let mut triangles = Vec::new();
            for triangle in mesh.descendants().filter(|n| n.has_tag_name("triangle")) {
                let mut indices = [0; 3];
                for (i, name) in ["v1", "v2", "v3"].iter().enumerate() {
                    indices[i] = triangle
                        .attribute(*name)
                        .ok_or("Missing triangle index")?
                        .parse()
                        .map_err(|_| "Invalid triangle index")?;
                    if indices[i] >= vertices.len() {
                        return Err("Triangle index outside mesh".into());
                    }
                }
                triangles.push(indices);
            }
            output.push(ModelMesh {
                vertices,
                triangles,
                build_item: 0,
            });
        }
        for component in object.descendants().filter(|n| n.has_tag_name("component")) {
            expand(component, objects, transforms, visiting, output)?;
        }
        transforms.pop();
        visiting.pop();
        Ok(())
    }
    let mut result = Vec::new();
    for (index, item) in doc
        .descendants()
        .filter(|n| n.has_tag_name("item"))
        .enumerate()
    {
        let before = result.len();
        expand(
            item,
            &objects,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut result,
        )?;
        for mesh in &mut result[before..] {
            mesh.build_item = index;
        }
    }
    Ok(result)
}
