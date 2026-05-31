use std::path::PathBuf;

fn main() {
    let initial_path = std::env::args().nth(1).map(|arg| {
        let p = PathBuf::from(arg);
        if p.is_absolute() {
            p
        } else {
            std::env::current_dir().unwrap().join(p)
        }
    });
    focus::chrome::run(initial_path);
}
