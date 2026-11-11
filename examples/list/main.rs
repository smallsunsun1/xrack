use list_ui::{
    AnchoredListDemo, DirectoryItem, GridDemo, ItemDelegatesDemo, LinkedScrollDemo, ListDemo,
    ListVisibilityDemo, NestedDataDemo, NestedListsDemo, ScrollBarsDemo, ScrollPositionDemo,
    StandaloneScrollDemo, StandardListDemo,
};

fn records() -> Vec<DirectoryItem> {
    (0..50_000)
        .map(|index| DirectoryItem {
            label: format!("条目 {index:05}"),
            rank: index,
        })
        .collect()
}

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--position") => xrack_platform_winit::run(ScrollPositionDemo::new()),
        Some("--linked") => xrack_platform_winit::run(LinkedScrollDemo::new()),
        Some("--scroll-bars") => xrack_platform_winit::run(ScrollBarsDemo::new()),
        Some("--standalone-scroll") => xrack_platform_winit::run(StandaloneScrollDemo::new()),
        Some("--item-delegates") => xrack_platform_winit::run(ItemDelegatesDemo::new()),
        Some("--visibility") => {
            let mut ui = ListVisibilityDemo::new();
            let records = ui.model_records();
            ui.on_reveal_all(move || {
                for index in 0..records.len() {
                    if let Some(mut record) = records.row(index)
                        && !record.flags.shown
                    {
                        record.flags.shown = true;
                        records.set_row(index, record);
                    }
                }
            });
            xrack_platform_winit::run(ui)
        }
        Some("--nested-data") => xrack_platform_winit::run(NestedDataDemo::new()),
        Some("--nested") => {
            let mut ui = NestedListsDemo::new();
            ui.set_albums(
                (0..10_000)
                    .map(|index| format!("分组 {index:05}"))
                    .collect(),
            );
            ui.set_tracks(
                (0..50_000)
                    .map(|index| format!("条目 {index:05}"))
                    .collect(),
            );
            xrack_platform_winit::run(ui)
        }
        Some("--anchor") => {
            let mut ui = AnchoredListDemo::new();
            ui.set_records(records());
            let model = ui.model_records();
            let prepend = model.clone();
            let serial = std::cell::Cell::new(0_i64);
            ui.on_prepend(move || {
                let rank = serial.get() - 1;
                serial.set(rank);
                prepend.insert(
                    0,
                    DirectoryItem {
                        label: format!("新增 {}", -rank),
                        rank,
                    },
                );
            });
            let descending = std::cell::Cell::new(true);
            ui.on_reverse_order(move || {
                let reverse = descending.get();
                descending.set(!reverse);
                model.sort_by(|a, b| {
                    if reverse {
                        b.rank.cmp(&a.rank)
                    } else {
                        a.rank.cmp(&b.rank)
                    }
                });
            });
            xrack_platform_winit::run(ui)
        }
        Some("--standard") => {
            let mut ui = StandardListDemo::new();
            ui.set_records(records());
            ui.set_jump_index(49_999);
            xrack_platform_winit::run(ui)
        }
        Some("--grid") => {
            let mut ui = GridDemo::new();
            ui.set_records(records());
            ui.set_jump_index(49_999);
            xrack_platform_winit::run(ui)
        }
        _ => xrack_platform_winit::run(ListDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
