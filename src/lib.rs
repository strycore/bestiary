//! bestiary — a catalog of Linux applications and where they keep their
//! data, across native / flatpak / snap / legacy install flavors.
//!
//! The data is the primary artifact. The Rust binary is a thin viewer over
//! the catalog; the library face is what other tools (fili, grimoire) consume.

pub mod catalog;
pub mod cli;
pub mod creature;

pub use catalog::{Catalog, CatalogEntry, Source};
pub use creature::{Creature, Dwelling, Flavor, Kind};
