//! `export-bindings <path>`: writes the TypeScript bindings of the commands.

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/lib/bindings.ts").into()
    });
    match app_lib::export_bindings(&path) {
        Ok(()) => println!("wrote {path}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
