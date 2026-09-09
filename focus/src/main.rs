use std::path::PathBuf;

fn main() {
    let initial_page = match std::env::args().nth(1) {
        Some(arg) if arg == "--launcher" => focus::chrome::InitialPage::Launcher,
        Some(arg) => {
            let path = PathBuf::from(arg);
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir().unwrap().join(path)
            };
            focus::chrome::InitialPage::File(path)
        }
        None => focus::chrome::InitialPage::Scratch,
    };
    focus::chrome::run(initial_page);
}
