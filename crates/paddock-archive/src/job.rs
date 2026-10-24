use crate::options::Options;
use crate::stage::extract;
use crate::writer::Report;
use paddock_jobs::{JobContext, JobError};
use std::path::Path;

/// Runs [`extract`] as the body of a `paddock-jobs` job: phases "Expanding" (xz only) and "Unpacking" in bytes, Cancel honoured
/// within a chunk, one terminal state. Register the job with `JobRegistry::register` and call this from a `JobPool::spawn_job` body.
///
/// ```ignore
/// let ctx = registry.register("unpack", "Unpacking the runtime", Some("llama"));
/// pool.spawn_job(ctx, move |ctx| paddock_archive::extract_job(ctx, &archive, &dest, &options))?;
/// ```
pub fn extract_job(
    ctx: &JobContext,
    archive: &Path,
    dest: &Path,
    opts: &Options,
) -> Result<Report, JobError> {
    extract(archive, dest, opts, ctx).map_err(JobError::from)
}
