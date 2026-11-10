//! Host sampling. E1 fills this module; C0 returns no sample.

use vowp_protocol::{HostSample, Timestamp};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

/// Samples the node and stores one row, when the sampler is implemented.
pub fn sample(_deps: &Deps, _store: &Store, _now: Timestamp) -> Result<Option<HostSample>, CoreError> {
    Ok(None)
}

/// Reads one host sample without storing it.
pub fn sample_host(_deps: &Deps) -> Result<HostSample, CoreError> {
    Err(CoreError::Invalid(
        "capacity sampler is not implemented in this build".to_owned(),
    ))
}

/// A running sampler thread. E1 owns the implementation.
pub struct SamplerHandle {
    _private: (),
}

impl SamplerHandle {
    /// Starts one sampler thread writing `host_samples` until cancelled.
    pub fn start(_deps: &Deps) -> Result<SamplerHandle, CoreError> {
        Err(CoreError::Invalid(
            "capacity sampler is not implemented in this build".to_owned(),
        ))
    }

    /// Stops the sampler thread.
    pub fn stop(self) -> Result<(), CoreError> {
        Ok(())
    }
}
