use std::{collections::HashMap, env, fs, io};
use wgpu_core::{
    command::{Command, PointerReferences, RenderCommand},
    device::trace::Action,
};

#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    calls: u64,
    draws_with_known_count: u64,
    draws_with_gpu_count: u64,
}

impl Counts {
    fn add(&mut self, other: Self) {
        self.calls += other.calls;
        self.draws_with_known_count += other.draws_with_known_count;
        self.draws_with_gpu_count += other.draws_with_gpu_count;
    }

    fn json(self) -> serde_json::Value {
        serde_json::json!({
            "drawCalls": self.calls,
            "drawsWithKnownCount": self.draws_with_known_count,
            "gpuCountedIndirectCalls": self.draws_with_gpu_count,
        })
    }
}

type TraceAction<'a> = Action<'a, PointerReferences>;
type Bundle = <PointerReferences as wgpu_core::command::ReferenceType>::RenderBundle;

fn render_counts(
    commands: &[RenderCommand<PointerReferences>],
    bundles: &HashMap<Bundle, Counts>,
) -> io::Result<Counts> {
    let mut count = Counts::default();
    for command in commands {
        match command {
            RenderCommand::Draw { .. }
            | RenderCommand::DrawIndexed { .. }
            | RenderCommand::DrawMeshTasks { .. } => {
                count.calls += 1;
                count.draws_with_known_count += 1;
            }
            RenderCommand::DrawIndirect { count: size, .. } => {
                count.calls += 1;
                count.draws_with_known_count += u64::from(*size);
            }
            RenderCommand::MultiDrawIndirectCount { .. } => {
                count.calls += 1;
                count.draws_with_gpu_count += 1;
            }
            RenderCommand::ExecuteBundle(id) => {
                count.add(
                    *bundles
                        .get(id)
                        .ok_or_else(|| io::Error::other("unknown trace render bundle"))?,
                );
            }
            _ => {}
        }
    }
    Ok(count)
}

fn summarize(actions: Vec<TraceAction<'_>>) -> io::Result<serde_json::Value> {
    let mut bundles = HashMap::new();
    let mut total = Counts::default();
    let mut interval = Counts::default();
    let mut presents = Vec::new();
    let mut submissions = Vec::new();
    for action in actions {
        match action {
            Action::CreateRenderBundle { id, base, .. } => {
                bundles.insert(id, render_counts(&base.commands, &bundles)?);
            }
            Action::DropRenderBundle(id) => {
                bundles.remove(&id);
            }
            Action::Submit(index, commands) => {
                let mut count = Counts::default();
                let mut passes = 0;
                let mut metal_present = false;
                for command in commands {
                    match command {
                        Command::RunRenderPass { pass, .. } => {
                            passes += 1;
                            count.add(render_counts(&pass.commands, &bundles)?);
                        }
                        Command::InsertDebugMarker(label)
                            if label == "XRack Metal presentation" =>
                        {
                            metal_present = true;
                        }
                        _ => {}
                    }
                }
                total.add(count);
                interval.add(count);
                submissions.push(serde_json::json!({
                    "submission": index,
                    "renderPasses": passes,
                    "draws": count.json(),
                }));
                if metal_present {
                    presents.push(std::mem::take(&mut interval).json());
                }
            }
            Action::Present(_) => presents.push(std::mem::take(&mut interval).json()),
            _ => {}
        }
    }
    Ok(serde_json::json!({
        "total": total.json(),
        "submissions": submissions,
        "presentationIntervals": presents,
        "trailingWork": interval.json(),
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = env::args_os()
        .nth(1)
        .ok_or_else(|| io::Error::other("usage: xrack-gpu-trace PATH/trace.ron"))?;
    let source = fs::read_to_string(file)?;
    let actions: Vec<TraceAction<'_>> = ron::from_str(&source)?;
    println!("{}", serde_json::to_string_pretty(&summarize(actions)?)?);
    Ok(())
}
