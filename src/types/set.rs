//! An unordered collection of unique members - the backing store for
//! the Redis-style Set data type.

use std::collections::HashSet;

use crate::util::bytes::Bytes;

pub type Set = HashSet<Bytes>;
