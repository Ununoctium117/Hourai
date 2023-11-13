use bitflags::bitflags;

bitflags! {
    /// A set of bitflags which can be used to specify what resource to process
    /// into the cache.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct ResourceType: u8 {
        const GUILD = 1;
        const MEMBER = 1 << 1;
        const PRESENCE = 1 << 2;
    }
}

/// Configuration for an [`InMemoryCache`].
///
/// [`InMemoryCache`]: crate::InMemoryCache
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub(super) resource_types: ResourceType,
}

impl Config {
    /// Returns a copy of the resource types enabled.
    pub fn resource_types(&self) -> ResourceType {
        self.resource_types.clone()
    }

    /// Returns a mutable reference to the resource types enabled.
    pub fn resource_types_mut(&mut self) -> &mut ResourceType {
        &mut self.resource_types
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            resource_types: ResourceType::all(),
        }
    }
}
