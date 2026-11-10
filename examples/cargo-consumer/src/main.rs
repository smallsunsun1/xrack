xrack::include_modules!();

fn main() {
    let mut counter = Counter::new();
    counter.dispatch("increment", "clicked");
    println!(
        "count={}, doubled={}",
        counter.get_count(),
        counter.get_doubled()
    );
}
