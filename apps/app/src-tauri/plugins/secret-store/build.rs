// Rust calls the secret functions directly; the page only asks for the
// system bar sizes (`insets`, mobile).
const COMMANDS: &[&str] = &["insets"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
