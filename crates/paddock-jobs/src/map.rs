use crate::cancel::CancelToken;
use crate::error::JobError;
use crate::pipeline::pipeline_ordered;
use crate::reporter::Counter;
use crate::workers::Workers;

/// Maps `items` on a bounded pool and returns the results **in input order**, whatever the scheduling. Adds 1 to `counter` per
/// finished item. The error of the lowest-index failing item wins (deterministic); a cancel returns `JobError::Cancelled`.
pub fn par_map_ordered<T, R, F>(
    items: Vec<T>,
    workers: Workers,
    cancel: &CancelToken,
    counter: &dyn Counter,
    f: F,
) -> Result<Vec<R>, JobError>
where
    T: Send,
    R: Send,
    F: Fn(T) -> Result<R, JobError> + Sync,
{
    let mut out = Vec::with_capacity(items.len());
    let window = workers.get() * 4;
    pipeline_ordered(items.into_iter(), workers, window, cancel, f, |r| {
        out.push(r);
        counter.add(1);
        Ok(())
    })?;
    Ok(out)
}
