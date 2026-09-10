//! End-of-replay gate for the timber reference example. This is a geometric
//! fabrication check, not a structural rating or timber movement certification.
use serde_json::Value;
use std::collections::BTreeMap;
type Box3 = [[f64; 2]; 3];
fn number(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .ok_or_else(|| "Expected numeric manufacturing input".into())
}
fn id(v: &Value) -> Result<u64, String> {
    v.as_u64()
        .ok_or_else(|| "Expected manufacturing reference ID".into())
}
fn array(v: &Value) -> Result<&Vec<Value>, String> {
    v.as_array()
        .ok_or_else(|| "Expected manufacturing input array".into())
}
fn vector(v: &Value) -> Result<[f64; 3], String> {
    if let Some(a) = v.as_array() {
        if a.len() == 3 {
            return Ok([number(&a[0])?, number(&a[1])?, number(&a[2])?]);
        }
    }
    Ok([number(&v["x"])?, number(&v["y"])?, number(&v["z"])?])
}
fn axis(v: &Value) -> Result<usize, String> {
    let i = id(v)? as usize;
    if i > 2 {
        return Err("Axis outside XYZ".into());
    }
    Ok(i)
}
fn require(yes: bool, why: &str) -> Result<(), String> {
    if yes {
        Ok(())
    } else {
        Err(format!("Manufacturing gate: {why}"))
    }
}
fn contact(a: Box3, b: Box3, axis: usize) -> Result<(), String> {
    require(
        (a[axis][1] - b[axis][0]).abs() < 1e-4,
        "bearing faces must meet",
    )?;
    for i in 0..3 {
        if i != axis {
            require(
                a[i][1].min(b[i][1]) - a[i][0].max(b[i][0]) >= 20.0 - 1e-5,
                "bearing width is below 20 mm",
            )?;
        }
    }
    Ok(())
}
pub(super) fn bench(exports: &Value) -> Result<(), String> {
    let inputs = &exports["verification_inputs"];
    let bodies = array(&exports["final_scene"]["bodies"])?;
    let occurrences = array(&inputs["occurrences"])?;
    let poses = array(&exports["final_solution"]["instance_body_poses"])?;
    let parts = array(&inputs["parts"])?;
    let mut boxes = BTreeMap::<u64, Box3>::new();
    for occurrence in occurrences {
        let occurrence_id = id(&occurrence["id"])?;
        let pose = poses
            .iter()
            .find(|p| p["occurrence_id"] == occurrence["id"])
            .ok_or("Missing solved occurrence")?;
        let translation = vector(&pose["translation"])?;
        let expected = vector(&occurrence["position"])?;
        require(
            translation
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-5),
            "assembly member moved from its designed location",
        )?;
        let rotation = array(&pose["rotation"])?;
        require(
            rotation.len() == 4
                && rotation[..3]
                    .iter()
                    .all(|v| v.as_f64().is_some_and(|v| v.abs() < 1e-6)),
            "timber stock axes must remain aligned",
        )?;
        let body = bodies
            .iter()
            .find(|b| b["id"] == occurrence["body_id"])
            .ok_or("Missing occurrence body")?;
        let vertices = array(&body["mesh"]["positions"])?;
        require(
            !vertices.is_empty() && vertices.len() % 3 == 0,
            "body has no complete mesh",
        )?;
        let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for (i, value) in vertices.iter().enumerate() {
            let axis = i % 3;
            let coordinate = number(value)? + translation[axis];
            bounds[axis][0] = bounds[axis][0].min(coordinate);
            bounds[axis][1] = bounds[axis][1].max(coordinate);
        }
        boxes.insert(occurrence_id, bounds);
    }
    for bore in array(&inputs["bores"])? {
        let body = bodies
            .iter()
            .find(|b| b["id"] == bore["body_id"])
            .ok_or("Missing drilled body")?;
        let axis = axis(&bore["axis"])?;
        let radius = number(&bore["diameter"])? / 2.0;
        for point in array(&bore["points"])? {
            let point = vector(point)?;
            let mut found = false;
            for face in array(&body["faces"])? {
                let cylinder = &face["cylinder"];
                if cylinder.is_null() {
                    continue;
                }
                let direction = vector(&cylinder["axis"])?;
                let origin = vector(&cylinder["origin"])?;
                if (direction[axis].abs() - 1.0).abs() < 1e-6
                    && (number(&cylinder["radius"])? - radius).abs() < 1e-6
                    && (0..3).all(|i| i == axis || (origin[i] - point[i]).abs() < 1e-5)
                {
                    found = true;
                    break;
                }
            }
            require(found, "a specified drilled axis moved or disappeared")?;
        }
    }
    let named = |needle: &str| -> Result<Vec<Box3>, String> {
        occurrences
            .iter()
            .filter(|o| {
                o["name"]
                    .as_str()
                    .is_some_and(|n| n.to_lowercase().contains(needle))
            })
            .map(|o| Ok(boxes[&id(&o["id"])?]))
            .collect()
    };
    let arms = named("armrest")?;
    let arm_rails = named("continuous arm support")?;
    let front = named("front leg")?;
    let seat_width = number(&inputs["design"]["seatWidth"])?;
    for pair in [&arms, &arm_rails, &front] {
        require(
            pair.len() == 2,
            "expected a pair of handed arm/frame members",
        )?;
        require(
            (pair[0][0][0] + pair[1][0][1] - seat_width).abs() < 1e-5
                && (pair[0][0][1] + pair[1][0][0] - seat_width).abs() < 1e-5,
            "left/right members must mirror about the seat center",
        )?;
    }
    for (i, arm) in arms.iter().enumerate() {
        require(
            (number(&inputs["design"]["rearPostFront"])?
                - arm[1][1]
                - number(&inputs["design"]["armRearClearance"])?)
            .abs()
                < 1e-5,
            "arm end must clear the rear post",
        )?;
        contact(front[i], *arm, 2)?;
        contact(arm_rails[i], *arm, 2)?;
    }
    for picket in named("back picket")? {
        for rail in named("back rail")? {
            contact(picket, rail, 1)?;
        }
    }
    for seat in named("seat slat")? {
        for support in named("upper side rail")?
            .into_iter()
            .chain(named("center seat bearer")?)
        {
            contact(support, seat, 2)?;
        }
    }
    let center = named("center seat bearer")?;
    require(center.len() == 1, "one center bearer required")?;
    for block in named("bearer support block")? {
        contact(block, center[0], 2)?;
    }
    require(
        number(&inputs["tolerances"]["post_notch_minimum_side_clearance"])? >= 1.0
            && number(&inputs["tolerances"]["arm_rear_minimum_clearance"])? >= 1.0,
        "machining tolerance stack must retain clearance",
    )?;
    let fasteners = array(&inputs["fasteners"])?;
    let stages = ["lower frame", "frame", "seat", "back", "arms"];
    let mut installed = BTreeMap::<u64, usize>::new();
    let mut shafts = Vec::<Box3>::new();
    for fastener in fasteners {
        let axis = axis(&fastener["axis"])?;
        let head = vector(&fastener["head"])?;
        let direction = number(&fastener["direction"])?;
        let length = number(&fastener["length"])?;
        let thickness = number(&fastener["thickness"])?;
        require(
            direction.abs() == 1.0,
            "screw direction must be signed unit axis",
        )?;
        let from = id(&fastener["from"])?;
        let to = id(&fastener["to"])?;
        let receiver = occurrences
            .iter()
            .find(|o| o["id"] == to)
            .ok_or("Missing receiver")?;
        let part = parts
            .iter()
            .find(|p| p["body_id"] == receiver["body_id"])
            .ok_or("Missing receiver stock")?;
        let size = vector(&part["stock_mm"])?;
        let source = boxes.get(&from).ok_or("Missing screw source")?;
        let receiving = boxes.get(&to).ok_or("Missing screw receiver")?;
        let entry = head[axis] + direction * thickness;
        require(
            (head[axis] - source[axis][if direction > 0.0 { 0 } else { 1 }]).abs() < 1e-5,
            "screw head must start on source face",
        )?;
        require(
            (entry - receiving[axis][if direction > 0.0 { 0 } else { 1 }]).abs() < 1e-5,
            "screw must enter receiving face",
        )?;
        require(
            length > thickness && length - thickness < size[axis] - 5.0,
            "screw must engage without tip protrusion",
        )?;
        let grain = id(&part["datums"]["grain_axis"])? as usize;
        require(axis != grain, "screw must enter side grain")?;
        for i in 0..3 {
            if i != axis {
                require(
                    head[i] > source[i][0] + 5.0
                        && head[i] < source[i][1] - 5.0
                        && head[i] > receiving[i][0] + 5.0
                        && head[i] < receiving[i][1] - 5.0,
                    "hole must retain a stock edge margin",
                )?;
            }
        }
        let stage = stages
            .iter()
            .position(|s| fastener["stage"] == *s)
            .ok_or("Unknown assembly stage")?;
        for member in [from, to] {
            installed
                .entry(member)
                .and_modify(|old| *old = (*old).min(stage))
                .or_insert(stage);
        }
        shafts.push(std::array::from_fn(|i| {
            let end = head[i] + if i == axis { direction * length } else { 0.0 };
            [head[i].min(end), head[i].max(end)]
        }));
    }
    for (i, a) in shafts.iter().enumerate() {
        for b in &shafts[i + 1..] {
            let distance = (0..3)
                .map(|axis| {
                    (a[axis][0] - b[axis][1])
                        .max(b[axis][0] - a[axis][1])
                        .max(0.0)
                        .powi(2)
                })
                .sum::<f64>();
            require(distance >= 5.5_f64.powi(2), "fastener shafts intersect")?;
        }
    }
    for fastener in fasteners {
        let head = vector(&fastener["head"])?;
        let axis = axis(&fastener["axis"])?;
        let direction = number(&fastener["direction"])?;
        let stage = stages
            .iter()
            .position(|s| fastener["stage"] == *s)
            .ok_or("Unknown assembly stage")?;
        let envelope: Box3 = std::array::from_fn(|i| {
            if i == axis {
                let end = head[i] - direction * 120.0;
                [head[i].min(end), head[i].max(end)]
            } else {
                [head[i] - 10.0, head[i] + 10.0]
            }
        });
        for (member, stock) in &boxes {
            if fastener["from"] == *member
                || fastener["to"] == *member
                || installed.get(member).is_none_or(|s| *s > stage)
            {
                continue;
            }
            require(
                !(0..3).all(|i| {
                    stock[i][1].min(envelope[i][1]) - stock[i][0].max(envelope[i][0]) > 0.01
                }),
                "assembly order obstructs the 120 mm driver envelope",
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bearing_rejects_gap_and_inadequate_overlap() {
        let a = [[0.0, 30.0], [0.0, 40.0], [0.0, 10.0]];
        let b = [[0.0, 30.0], [0.0, 40.0], [10.0, 20.0]];
        assert!(contact(a, b, 2).is_ok());
        let mut shifted = b;
        shifted[2][0] = 11.0;
        assert!(contact(a, shifted, 2).is_err());
        shifted = b;
        shifted[0] = [20.0, 50.0];
        assert!(contact(a, shifted, 2).is_err());
    }
}
