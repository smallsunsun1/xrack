mod ui {
    include!(concat!(env!("OUT_DIR"), "/ui.rs"));
}

fn main() {
    let mut counter = ui::Counter::new();
    counter.dispatch("increment", "clicked");
    println!(
        "count={}, doubled={}",
        counter.get_count(),
        counter.get_doubled()
    );
}
