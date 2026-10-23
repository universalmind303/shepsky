//! Shared core for collie: herdr wire types, the session store, and
//! notification logic. Pure data + logic — no I/O — so the Mac bridge and the
//! Android runtime use the exact same brain.

pub mod herdr;
pub mod notify;
pub mod store;
