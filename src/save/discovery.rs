use std::any::Any;

/// Trait for components that participate in the save system.
/// Derived via `#[derive(Saveable)]` from the proc-macro crate.
pub trait Saveable: Any + Send + Sync {
    fn type_name(&self) -> &'static str;
}

/// Registry that auto-discovers all Saveable types.
#[derive(Default)]
pub struct SaveRegistry {
    pub type_names: Vec<&'static str>,
}
