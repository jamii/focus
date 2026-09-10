pub mod app;
pub mod buffer;
pub mod drawing;
pub mod editor;
pub mod fuzz;
mod fuzz_gen;
mod fuzzy;
pub mod input;
pub(crate) mod map;
pub mod page;
pub mod style;
pub mod window;

#[macro_export]
macro_rules! log {
    ($io:expr, $($arg:tt)*) => {
        ($io).log(::std::format_args!($($arg)*))
    };
}
