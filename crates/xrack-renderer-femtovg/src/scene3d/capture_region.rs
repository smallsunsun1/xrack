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
        let Some(pixels) = pixel_rect(clip.bounds().map(f64::from), size) else {
            return Self::Empty;
        };
        if viewport.is_none() && pixels == [0, 0, size[0], size[1]] {
            Self::Full
        } else {
            Self::Restricted { viewport, pixels }
        }
    }

    pub fn scissor(bounds: Option<[f64; 4]>, size: [u32; 2]) -> Self {
        let Some(bounds) = bounds else {
            return Self::Full;
        };
        match pixel_rect(bounds, size) {
            None => Self::Empty,
            Some(pixels) if pixels == [0, 0, size[0], size[1]] => Self::Full,
            Some(pixels) => Self::Restricted {
                viewport: None,
                pixels,
            },
        }
    }

    pub fn pixels(self, [width, height]: [u32; 2]) -> Option<[u32; 4]> {
        match self {
            Self::Full => Some([0, 0, width, height]),
            Self::Empty => None,
            Self::Restricted { pixels, .. } => Some(pixels),
        }
    }

    pub fn translated(self, [x, y]: [u32; 2]) -> Self {
        match self {
            Self::Restricted {
                viewport,
                pixels: [left, top, width, height],
            } => Self::Restricted {
                viewport,
                pixels: [left + x, top + y, width, height],
            },
            _ => self,
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

fn pixel_rect([left, top, right, bottom]: [f64; 4], [width, height]: [u32; 2]) -> Option<[u32; 4]> {
    // Include the pixel centers within the half-open rectangle. Adjacent
    // regions then neither overlap nor leave a gap at fractional scales.
    let edge = |unit: f64, size: u32| {
        (unit * f64::from(size) - 0.5)
            .ceil()
            .clamp(0.0, f64::from(size)) as u32
    };
    let x = edge(left, width);
    let y = edge(top, height);
    let width = edge(right, width).saturating_sub(x);
    let height = edge(bottom, height).saturating_sub(y);
    (width != 0 && height != 0).then_some([x, y, width, height])
}
