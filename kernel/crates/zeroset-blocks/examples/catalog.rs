use zeroset_blocks::registry::BlockRegistry;

fn main() {
    let catalog = BlockRegistry::builtin();
    println!("{}", serde_json::to_string_pretty(catalog.defs()).unwrap());
}
