//! A field -> value map - the backing store for the Redis-style Hash
//! data type. `std::collections::HashMap` replaces the hand-rolled
//! FNV-1a chaining table the C version used.

use std::collections::HashMap;

pub type Hash = HashMap<String, String>;
