fn main() {
    #[cfg(feature = "viewer")]
    tauri_build::build();
}
