//! An unordered collection of unique string members - the backing store
//! for the Redis-style Set data type.

use std::collections::HashSet;

pub type Set = HashSet<String>;
