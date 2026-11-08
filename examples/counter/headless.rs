use counter_ui::Counter;

fn main() {
    let mut counter = Counter::new();
    counter.invoke_increment(2);
    counter.dispatch("add", "clicked");
    println!(
        "count={}, doubled={}",
        counter.get_count(),
        counter.get_doubled()
    );
    println!("scene={:#?}", counter.render());
}
