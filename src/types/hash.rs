//! A field -> value map - the backing store for the Redis-style Hash
//! data type. Both halves are raw bytes, so a field or value may hold
//! anything a client sends.

use std::collections::HashMap;

use crate::util::bytes::Bytes;

pub type Hash = HashMap<Bytes, Bytes>;
