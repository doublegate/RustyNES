fn main() {
    if let Some(d) = directories::ProjectDirs::from("io.github", "doublegate", "RustySNES") {
        println!("config: {:?}", d.config_dir());
        println!("data: {:?}", d.data_dir());
    }
}
