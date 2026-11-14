//! VOWP transport boundary: the trait and its live/mock implementations.

pub mod client;

pub use client::{MockVowpClient, SshVowpClient, VowpClient, VowpProbe};
