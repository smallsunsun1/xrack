//! Counters for an opt-in native UI update on the current thread.
use std::{cell::RefCell, time::Instant};

const MAX_VIEWS: usize = 24;
const MAX_BINDINGS: usize = 48;

#[derive(Clone, Debug, Default)]
pub struct UpdateSample {
    pub property_evaluations: usize,
    pub property_cache_hits: usize,
    pub layout_runs: usize,
    /// Total of direct reuse and reprojection, for compatibility.
    pub layout_reuses: usize,
    pub layout_direct_reuses: usize,
    pub layout_reprojections: usize,
    pub materialized_rows: usize,
    pub model_rows_scanned: usize,
    pub model_rows_cloned: usize,
    pub scene_projections_built: usize,
    pub scene_projection_cache_hits: usize,
    pub scene_lod_groups_evaluated: usize,
    pub scene_lod_view_cache_hits: usize,
    pub draw_primitives: usize,
    pub quad_batches: usize,
    pub glyph_batches: usize,
    pub vertex_bytes: usize,
    pub gpu_submissions: usize,
    pub window_image_transfers: usize,
    pub window_geometry_builds: usize,
    pub texture_upload_bytes: usize,
    pub geometry_upload_bytes: usize,
    pub geometry_reuses: usize,
    pub cpu_micros: u64,
    pub view_rows: Vec<(String, usize)>,
    pub visual_binding_reads: Vec<(String, &'static str, usize)>,
    pub visual_binding_reads_truncated: bool,
}

thread_local! {
    static CURRENT: RefCell<Option<UpdateSample>> = const { RefCell::new(None) };
}

pub fn measure(update: impl FnOnce()) -> Option<UpdateSample> {
    CURRENT.with(|current| {
        if current.borrow().is_some() {
            update();
            return None;
        }
        struct Clear<'a>(&'a RefCell<Option<UpdateSample>>);
        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                self.0.replace(None);
            }
        }
        let _clear = Clear(current);
        current.replace(Some(UpdateSample::default()));
        let start = Instant::now();
        update();
        let mut sample = current.take()?;
        sample.cpu_micros = u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);
        tracing::debug!(
            target: crate::ui_trace::TARGET,
            event = "ui_update",
            evaluations = sample.property_evaluations as u64,
            cache_hits = sample.property_cache_hits as u64,
            layout_runs = sample.layout_runs as u64,
            layout_reuses = sample.layout_reuses as u64,
            layout_direct_reuses = sample.layout_direct_reuses as u64,
            layout_reprojections = sample.layout_reprojections as u64,
            materialized_rows = sample.materialized_rows as u64,
            scene_projections_built = sample.scene_projections_built as u64,
            scene_projection_cache_hits = sample.scene_projection_cache_hits as u64,
            scene_lod_groups_evaluated = sample.scene_lod_groups_evaluated as u64,
            scene_lod_view_cache_hits = sample.scene_lod_view_cache_hits as u64,
            cpu_micros = sample.cpu_micros,
        );
        Some(sample)
    })
}

#[inline]
fn count(update: impl FnOnce(&mut UpdateSample)) {
    CURRENT.with(|current| {
        if let Some(sample) = current.borrow_mut().as_mut() {
            update(sample);
        }
    });
}

pub(crate) fn property(evaluated: bool) {
    count(|sample| {
        if evaluated {
            sample.property_evaluations += 1;
        } else {
            sample.property_cache_hits += 1;
        }
    });
}

pub fn layout(reused: bool) {
    count(|sample| {
        if reused {
            sample.layout_reuses += 1;
            sample.layout_direct_reuses += 1;
        } else {
            sample.layout_runs += 1;
        }
    });
}

pub fn layout_reprojected() {
    count(|sample| {
        sample.layout_reuses += 1;
        sample.layout_reprojections += 1;
    });
}

pub fn rows(view: Option<&str>, counted: usize) {
    count(|sample| {
        sample.materialized_rows += counted;
        if let Some(view) = view {
            if let Some((_, rows)) = sample.view_rows.iter_mut().find(|(name, _)| name == view) {
                *rows += counted;
            } else if sample.view_rows.len() < MAX_VIEWS {
                sample.view_rows.push((view.to_owned(), counted));
            }
        }
    });
}

pub fn visual_binding(element: &str, property: &'static str) {
    if element.is_empty() {
        return;
    }
    count(|sample| {
        let entries = &mut sample.visual_binding_reads;
        if let Some((_, _, reads)) = entries
            .iter_mut()
            .find(|(id, name, _)| id == element && *name == property)
        {
            *reads += 1;
        } else if entries.len() < MAX_BINDINGS {
            entries.push((element.to_owned(), property, 1));
        } else {
            sample.visual_binding_reads_truncated = true;
        }
    });
}

#[doc(hidden)]
pub fn model_work(scanned: usize, cloned: usize) {
    count(|sample| {
        sample.model_rows_scanned += scanned;
        sample.model_rows_cloned += cloned;
    });
}

pub(crate) fn scene_projection(reused: bool) {
    count(|sample| {
        if reused {
            sample.scene_projection_cache_hits += 1;
        } else {
            sample.scene_projections_built += 1;
        }
    });
}

pub(crate) fn scene_lod_groups_evaluated(groups: usize) {
    count(|sample| sample.scene_lod_groups_evaluated += groups);
}

pub(crate) fn scene_lod_view_reused() {
    count(|sample| sample.scene_lod_view_cache_hits += 1);
}

#[doc(hidden)]
pub fn draw_primitive() {
    count(|sample| sample.draw_primitives += 1);
}

#[doc(hidden)]
pub fn submit_gpu() {
    count(|sample| sample.gpu_submissions += 1);
}

#[doc(hidden)]
pub fn build_window_geometry() {
    count(|sample| sample.window_geometry_builds += 1);
}

#[doc(hidden)]
pub fn transfer_window_image() {
    count(|sample| sample.window_image_transfers += 1);
}

#[doc(hidden)]
pub fn texture_upload(bytes: usize) {
    count(|sample| sample.texture_upload_bytes += bytes);
}

#[doc(hidden)]
pub fn quad_batch(quads: usize) {
    // Six XYUV vertices per quad in FemtoVG's triangle command.
    quad_batch_upload(quads * 6 * 4 * std::mem::size_of::<f32>());
}

#[doc(hidden)]
pub fn quad_batch_upload(bytes: usize) {
    count(|sample| {
        sample.quad_batches += 1;
        sample.vertex_bytes += bytes;
    });
}

#[doc(hidden)]
pub fn glyph_batch() {
    count(|sample| sample.glyph_batches += 1);
}

#[doc(hidden)]
pub fn geometry_upload(bytes: usize) {
    count(|sample| sample.geometry_upload_bytes += bytes);
}

#[doc(hidden)]
pub fn reused_geometry() {
    count(|sample| sample.geometry_reuses += 1);
}
