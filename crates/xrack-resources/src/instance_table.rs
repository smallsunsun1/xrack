//! Decode the XML and QtIR resource formats used by Qt Quick 3D FileInstancing.

use nalgebra::{Quaternion, UnitQuaternion, Vector3};
use xrack_runtime::{
    Color,
    scene3d::{InstanceTable3D, MAX_MESH_INSTANCES, linear_color},
};

pub(super) fn decode(bytes: &[u8], binary: bool) -> Result<InstanceTable3D, String> {
    if binary {
        decode_binary(bytes)
    } else {
        decode_xml(bytes)
    }
}

fn decode_binary(bytes: &[u8]) -> Result<InstanceTable3D, String> {
    const HEADER: usize = 20;
    let header: &[u8; HEADER] = bytes.first_chunk().ok_or("truncated QtIR header")?;
    if &header[..4] != b"QtIR" {
        return Err("invalid QtIR magic".into());
    }
    let major = u16::from_le_bytes([header[4], header[5]]);
    let minor = u16::from_le_bytes([header[6], header[7]]);
    if (major, minor) != (1, 0) {
        return Err(format!("unsupported QtIR version {major}.{minor}"));
    }
    let (words, _) = header.as_chunks::<4>();
    let stride = u32::from_le_bytes(words[2]) as usize;
    let offset = u32::from_le_bytes(words[3]) as usize;
    let count = u32::from_le_bytes(words[4]) as usize;
    if stride != 80 || offset != HEADER || count > MAX_MESH_INSTANCES {
        return Err("invalid QtIR stride, offset or instance count".into());
    }
    if bytes.len() != offset + stride * count {
        return Err("QtIR length does not match its instance count".into());
    }
    InstanceTable3D::from_packed(&bytes[offset..]).ok_or_else(|| {
        "QtIR rows require finite data, linear RGBA and nonsingular transforms".into()
    })
}

fn decode_xml(bytes: &[u8]) -> Result<InstanceTable3D, String> {
    let xml = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let document = roxmltree::Document::parse(xml).map_err(|error| error.to_string())?;
    let root = document.root_element();
    if !root.has_tag_name("InstanceTable") {
        return Err("expected an InstanceTable root element".into());
    }
    let mut entries = Vec::new();
    for node in root.children().filter(|node| node.has_tag_name("Instance")) {
        if entries.len() == MAX_MESH_INSTANCES {
            return Err(format!(
                "instance table has more than {MAX_MESH_INSTANCES} rows"
            ));
        }
        let row =
            decode_row(node).map_err(|error| format!("Instance {}: {error}", entries.len()))?;
        entries.push(row);
    }
    InstanceTable3D::from_rows(entries.into_iter())
        .ok_or_else(|| "instance table requires finite data and nonsingular transforms".into())
}

fn decode_row(node: roxmltree::Node<'_, '_>) -> Result<[[f32; 4]; 5], String> {
    let position = channels(node, "position", [0.0; 3])?;
    let scale = channels(node, "scale", [1.0; 3])?;
    let orientation = if node.attribute("quaternion").is_some() {
        let [x, y, z, w] = channels(node, "quaternion", [0.0, 0.0, 0.0, 1.0])?;
        UnitQuaternion::try_new(Quaternion::new(w, x, y, z), 1e-8).ok_or("invalid quaternion")?
    } else {
        let [pitch, yaw, roll] = channels(node, "eulerRotation", [0.0; 3])?.map(f32::to_radians);
        UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), pitch)
            * UnitQuaternion::from_axis_angle(&Vector3::z_axis(), roll)
    };
    let mut rows = [[0.0; 4]; 5];
    let rotation = orientation.to_rotation_matrix();
    for row in 0..3 {
        for column in 0..3 {
            rows[row][column] = rotation[(row, column)] * scale[column];
        }
        rows[row][3] = position[row];
    }
    rows[3] = node
        .attribute("color")
        .map_or_else(|| Ok([1.0; 4]), color)?;
    rows[4] = channels(node, "custom", [0.0; 4])?;
    Ok(rows)
}

fn channels<const N: usize>(
    node: roxmltree::Node<'_, '_>,
    name: &str,
    fallback: [f32; N],
) -> Result<[f32; N], String> {
    let Some(source) = node.attribute(name) else {
        return Ok(fallback);
    };
    let mut result = [0.0; N];
    for (position, word) in source.split_ascii_whitespace().enumerate() {
        let target = result
            .get_mut(position)
            .ok_or_else(|| format!("{name} accepts at most {N} components"))?;
        *target = word
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("{name} requires finite numeric components"))?;
    }
    Ok(result)
}

fn color(source: &str) -> Result<[f32; 4], String> {
    let color = if let Some(argb) = source.strip_prefix('#').filter(|hex| hex.len() == 8) {
        Color(
            u32::from_str_radix(argb, 16)
                .map_err(|_| format!("invalid color {source:?}"))?
                .rotate_left(8),
        )
    } else {
        let svg = source
            .parse::<svgtypes::Color>()
            .map_err(|_| format!("invalid color {source:?}"))?;
        Color(u32::from_be_bytes([
            svg.red, svg.green, svg.blue, svg.alpha,
        ]))
    };
    let [r, g, b] = linear_color(color);
    Ok([r, g, b, f32::from(color.0 as u8) / 255.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(table: &InstanceTable3D) -> Vec<u8> {
        let mut bytes = b"QtIR\x01\0\0\0".to_vec();
        bytes.extend(
            [80, 20, table.len() as u32]
                .into_iter()
                .flat_map(u32::to_le_bytes),
        );
        bytes.extend(table.packed_words().flat_map(f32::to_le_bytes));
        bytes
    }

    #[test]
    fn xml_and_qtir_preserve_transforms_tint_and_custom_channels() {
        let table = decode(
            br##"<?xml version="1.0"?>
            <InstanceTable>
                <Instance position="3 4" scale="-2 3 4" eulerRotation="90 0 0"
                    quaternion="0 0 1 0" color="#80ff0000" custom="9 8"/>
                <Ignore><Instance position="30"/></Ignore>
                <Instance eulerRotation="0 0 90" color="lime" />
            </InstanceTable>"##,
            false,
        )
        .expect("valid instance table");
        let packed: Vec<_> = table.packed_words().collect();
        assert_eq!(table.len(), 2);
        assert_eq!(
            &packed[..20],
            &[
                2.0,
                0.0,
                0.0,
                3.0,
                0.0,
                -3.0,
                0.0,
                4.0,
                0.0,
                0.0,
                4.0,
                0.0,
                1.0,
                0.0,
                0.0,
                128.0 / 255.0,
                9.0,
                8.0,
                0.0,
                0.0
            ]
        );
        assert!((packed[21] + 1.0).abs() < 1e-6);
        assert!((packed[24] - 1.0).abs() < 1e-6);
        assert_eq!(&packed[32..36], &[0.0, 1.0, 0.0, 1.0]);
        assert_eq!(decode(&binary(&table), true).expect("valid QtIR"), table);
        let empty = decode(b"<InstanceTable/>", false).expect("empty XML table");
        assert!(empty.is_empty() && empty.is_loaded());
        assert_eq!(decode(&binary(&empty), true).expect("empty QtIR"), empty);
        assert_ne!(empty, InstanceTable3D::default());
    }

    #[test]
    fn malformed_table_headers_and_rows_are_rejected() {
        for xml in [
            r#"<Rows/>"#,
            r#"<InstanceTable><Instance position="NaN"/></InstanceTable>"#,
            r#"<InstanceTable><Instance quaternion="0 0 0 0"/></InstanceTable>"#,
            r#"<InstanceTable><Instance scale="0 1 1"/></InstanceTable>"#,
        ] {
            assert!(decode(xml.as_bytes(), false).is_err(), "{xml}");
        }
        let table =
            decode(b"<InstanceTable><Instance/></InstanceTable>", false).expect("identity entry");
        let bytes = binary(&table);
        for (offset, bad) in [(0, b'R'), (4, 2), (8, 79), (12, 16), (16, 2)] {
            let mut wrong = bytes.clone();
            wrong[offset] = bad;
            assert!(decode(&wrong, true).is_err(), "header byte {offset}");
        }
        let mut nan = bytes.clone();
        nan[20..24].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode(&nan, true).is_err());
        assert!(decode(&bytes[..bytes.len() - 1], true).is_err());
    }
}
