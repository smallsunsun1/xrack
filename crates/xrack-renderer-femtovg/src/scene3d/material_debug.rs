use xrack_runtime::Node;

// Mode values match the material diagnostics in scene.wgsl.
#[repr(u32)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum MaterialDebug {
    None,
    BaseColor,
    Roughness,
    Metalness,
    Emission,
    AmbientOcclusion,
    Normals,
    Tangents,
    Binormals,
    F0,
    Diffuse,
    Specular,
    ShadowOcclusion,
}

impl MaterialDebug {
    pub fn from_node(node: &Node) -> Self {
        match node.text("material-debug") {
            Some("base-color") => Self::BaseColor,
            Some("roughness") => Self::Roughness,
            Some("metalness") => Self::Metalness,
            Some("emission") => Self::Emission,
            Some("ambient-occlusion") => Self::AmbientOcclusion,
            Some("normals") => Self::Normals,
            Some("tangents") => Self::Tangents,
            Some("binormals") => Self::Binormals,
            Some("f0") => Self::F0,
            Some("diffuse") => Self::Diffuse,
            Some("specular") => Self::Specular,
            Some("shadow-occlusion") => Self::ShadowOcclusion,
            _ => Self::None,
        }
    }

    pub fn active(self) -> bool {
        self != Self::None
    }
}
