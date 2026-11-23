//! Turn linear grayscale samples into a static, pickable terrain mesh.
use crate::assets::read_asset;
use nalgebra::Vector3;
use png::{BitDepth, ColorType, Decoder};
use std::{io::Cursor, path::Path};
use xrack_runtime::scene3d::{
    MAX_IMPORTED_VERTICES, Mesh3D, MeshAlphaMode, MeshAsset, MeshMaterial, MeshPart, MeshPbr,
    MeshVertex,
};

pub(super) fn load(path: &Path) -> Result<MeshAsset, String> {
    let bytes = read_asset(path, "heightfield")?;
    decode(&bytes).map(MeshAsset::Static)
}

fn decode(bytes: &[u8]) -> Result<Mesh3D, String> {
    let mut decoder = Decoder::new(Cursor::new(bytes));
    let header = decoder
        .read_header_info()
        .map_err(|error| error.to_string())?;
    if header.color_type != ColorType::Grayscale
        || !matches!(header.bit_depth, BitDepth::Eight | BitDepth::Sixteen)
    {
        return Err("heightfield requires an 8-bit or 16-bit grayscale PNG".into());
    }
    let (width, height) = (header.width, header.height);
    if width < 2 || height < 2 {
        return Err("heightfield requires at least two samples along each axis".into());
    }
    let cells = u64::from(width - 1) * u64::from(height - 1);
    if cells > MAX_IMPORTED_VERTICES as u64 / 6 {
        return Err(format!(
            "heightfield {width} × {height} exceeds the mesh budget of {} grid cells",
            MAX_IMPORTED_VERTICES / 6
        ));
    }
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    if reader.info().animation_control.is_some() {
        return Err("heightfield requires a single PNG frame".into());
    }
    let mut bytes = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or("invalid heightfield PNG size")?
    ];
    let frame = reader
        .next_frame(&mut bytes)
        .map_err(|error| error.to_string())?;
    let samples: Vec<f32> = if frame.bit_depth == BitDepth::Sixteen {
        bytes[..frame.buffer_size()]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|value| f32::from(u16::from_be_bytes(*value)) / 65535.0)
            .collect()
    } else {
        bytes[..frame.buffer_size()]
            .iter()
            .map(|&value| f32::from(value) / 255.0)
            .collect()
    };
    let (width, height) = (width as usize, height as usize);
    if samples.len() != width * height {
        return Err("heightfield sample count does not match its dimensions".into());
    }

    let sample = |x, z| samples[z * width + x];
    let x_step = 1.0 / (width - 1) as f32;
    let z_step = 1.0 / (height - 1) as f32;
    let mut grid = Vec::with_capacity(samples.len());
    for z in 0..height {
        let top = z.saturating_sub(1);
        let bottom = (z + 1).min(height - 1);
        let v = z as f32 / (height - 1) as f32;
        for x in 0..width {
            let left = x.saturating_sub(1);
            let right = (x + 1).min(width - 1);
            let u = x as f32 / (width - 1) as f32;
            let dx = (sample(right, z) - sample(left, z)) / ((right - left) as f32 * x_step);
            let dz = (sample(x, bottom) - sample(x, top)) / ((bottom - top) as f32 * z_step);
            grid.push(MeshVertex {
                position: [u - 0.5, sample(x, z), v - 0.5],
                normal: Vector3::new(-dx, 1.0, -dz).normalize().into(),
                uv: [u, v],
                uv1: [0.0; 2],
                color: [1.0; 4],
            });
        }
    }
    let mut vertices = Vec::with_capacity(cells as usize * 6);
    for z in 0..height - 1 {
        for x in 0..width - 1 {
            let i = z * width + x;
            let a = grid[i];
            let b = grid[i + 1];
            let c = grid[i + width];
            let d = grid[i + width + 1];
            vertices.extend([a, c, b, b, c, d]);
        }
    }

    let count = vertices.len();
    Mesh3D::imported(
        vertices,
        Vec::new(),
        vec![MeshPart {
            vertices: 0..count,
            material: 0,
            volume_inverse: None,
        }],
        vec![MeshMaterial {
            base_color: [1.0; 4],
            base_texture: None,
            alpha_mode: MeshAlphaMode::Opaque,
            double_sided: false,
            pbr: Some(MeshPbr {
                metallic: 0.0,
                roughness: 1.0,
                emissive: [0.0; 3],
                occlusion_strength: 1.0,
                metallic_roughness_texture: None,
                emissive_texture: None,
                occlusion_texture: None,
                normal_map: None,
                clearcoat: None,
                dielectric: None,
                transmission: None,
            }),
        }],
    )
    .map_err(str::to_owned)
}

pub(super) fn generate_lightmap_uv(mesh: MeshAsset) -> Result<MeshAsset, String> {
    let MeshAsset::Static(mesh) = mesh else {
        return Err("heightfield requires a static mesh".into());
    };
    // A heightfield has exactly one sample at each XZ coordinate. The complete
    // mesh fits one chart even when the surface has very steep slopes.
    let coordinates = mesh.vertices().iter().map(|vertex| vertex.uv).collect();
    mesh.with_generated_lightmap_uv(coordinates, 0)
        .map(MeshAsset::Static)
        .ok_or_else(|| "could not store heightfield lightmap coordinates".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(
        size: [u32; 2],
        color: ColorType,
        depth: BitDepth,
        samples: &[u8],
    ) -> Result<Vec<u8>, png::EncodingError> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, size[0], size[1]);
        encoder.set_color(color);
        encoder.set_depth(depth);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(samples)?;
        writer.finish()?;
        Ok(bytes)
    }

    #[test]
    fn png_heightfield_preserves_sixteen_bit_samples_and_front_face_orientation()
    -> Result<(), Box<dyn std::error::Error>> {
        let sample = [16_384u16, 16_385, 32_768, 65_535]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let asset = png([2, 2], ColorType::Grayscale, BitDepth::Sixteen, &sample)?;
        let mesh = decode(&asset)?;
        let [a, c, b, b_again, c_again, d] = mesh.vertices() else {
            return Err("each grid cell should produce exactly two triangles".into());
        };
        assert_eq!(a.position, [-0.5, 16384.0 / 65535.0, -0.5]);
        assert_eq!(b.position, [0.5, 16385.0 / 65535.0, -0.5]);
        assert!(a.position[1] < b.position[1]);
        assert_eq!(d.position, [0.5, 1.0, 0.5]);
        assert_eq!(b.normal, b_again.normal);
        assert_eq!(c.normal, c_again.normal);
        let a = Vector3::from(a.position);
        assert!(
            (Vector3::from(c.position) - a)
                .cross(&(Vector3::from(b.position) - a))
                .y
                > 0.0
        );
        let MeshAsset::Static(chart) = generate_lightmap_uv(MeshAsset::Static(mesh))? else {
            return Err("heightfield charts should keep a static mesh".into());
        };
        assert_eq!(
            chart.lightmap_uvs(),
            &[
                [0.0, 0.0],
                [0.0, 1.0],
                [1.0, 0.0],
                [1.0, 0.0],
                [0.0, 1.0],
                [1.0, 1.0]
            ]
        );
        Ok(())
    }

    #[test]
    fn png_heightfield_requires_grayscale_and_checks_vertex_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        let rgb = png([2, 2], ColorType::Rgb, BitDepth::Eight, &[0; 12])?;
        assert!(decode(&rgb).unwrap_err().contains("grayscale PNG"));
        let line = png([1, 3], ColorType::Grayscale, BitDepth::Eight, &[0; 3])?;
        assert!(decode(&line).unwrap_err().contains("along each axis"));
        let too_large = png(
            [512, 512],
            ColorType::Grayscale,
            BitDepth::Eight,
            &vec![0; 512 * 512],
        )?;
        assert!(decode(&too_large).unwrap_err().contains("mesh budget"));
        let grayscale = png(
            [2, 2],
            ColorType::Grayscale,
            BitDepth::Eight,
            &[0, 127, 255, 255],
        )?;
        assert_eq!(decode(&grayscale)?.vertices()[2].position[1], 127.0 / 255.0);
        Ok(())
    }
}
