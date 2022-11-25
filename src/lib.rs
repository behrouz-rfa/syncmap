mod reclaim;
mod entry;
pub mod map;



/// Default hasher for [`HashMap`].
pub type DefaultHashBuilder = ahash::RandomState;
