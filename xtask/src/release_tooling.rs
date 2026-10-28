pub mod advisory_exceptions;
pub mod tag;
pub mod version;

pub fn root() -> &'static std::path::Path {
    crate::build_tools::root()
}
