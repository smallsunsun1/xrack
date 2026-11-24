use xrack_runtime::{
    scene_effect::{CaptureRasterSettings, CaptureRect},
    scene3d::SceneCamera,
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum CaptureRegion {
    #[default]
    Full,
    Empty,
    Restricted {
        viewport: Option<CaptureRect>,
        pixels: [u32; 4],
    },
}

impl CaptureRegion {
    pub fn new(settings: CaptureRasterSettings, size: [u32; 2]) -> Self {
        let viewport = settings
            .viewport
            .filter(|viewport| *viewport != CaptureRect::FULL);
        let clip = viewport
            .unwrap_or(CaptureRect::FULL)
            .intersect(settings.scissor.unwrap_or(CaptureRect::FULL));
        if clip == CaptureRect::FULL {
            return Self::Full;
        }
        let Some(pixels) = pixel_rect(clip, size) else {
            return Self::Empty;
        };
        if viewport.is_none() && pixels == [0, 0, size[0], size[1]] {
            Self::Full
        } else {
            Self::Restricted { viewport, pixels }
        }
    }

    pub fn is_empty(self) -> bool {
        self == Self::Empty
    }

    pub fn maps_viewport(self) -> bool {
        matches!(
            self,
            Self::Restricted {
                viewport: Some(_),
                ..
            }
        )
    }

    pub fn camera(self, camera: SceneCamera) -> SceneCamera {
        match self {
            Self::Restricted {
                viewport: Some(viewport),
                ..
            } => camera.with_clip_viewport(viewport.bounds()),
            _ => camera,
        }
    }

    pub fn clip(self, pass: &mut wgpu::RenderPass<'_>) -> bool {
        match self {
            Self::Full => true,
            Self::Empty => false,
            Self::Restricted {
                pixels: [x, y, width, height],
                ..
            } => {
                pass.set_scissor_rect(x, y, width, height);
                true
            }
        }
    }
}

fn pixel_rect(rect: CaptureRect, [width, height]: [u32; 2]) -> Option<[u32; 4]> {
    let [left, top, right, bottom] = rect.bounds();
    // Include the pixel centers within the half-open rectangle. Adjacent
    // regions then neither overlap nor leave a gap at fractional scales.
    let edge = |unit: f32, size: u32| {
        (f64::from(unit) * f64::from(size) - 0.5)
            .ceil()
            .clamp(0.0, f64::from(size)) as u32
    };
    let x = edge(left, width);
    let y = edge(top, height);
    let width = edge(right, width).saturating_sub(x);
    let height = edge(bottom, height).saturating_sub(y);
    (width != 0 && height != 0).then_some([x, y, width, height])
}
