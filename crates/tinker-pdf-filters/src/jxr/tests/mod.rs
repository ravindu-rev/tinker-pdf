//! In-crate tests for the JPEG XR decoder.
//!
//! Split the way `jpx/tests/` is: the container and header readers get their
//! own file, and the refusal list gets one whose whole job is to reach every
//! variant — "the refusals are the feature" is only a claim if something
//! checks that they fire.

mod containers;
mod fixtures;
mod refusals;
mod writer;
