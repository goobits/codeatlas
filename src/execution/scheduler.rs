use super::budget::CallBudget;
use super::cancellation::{register_cancellation, CancellationRegistration};
use super::model::{ExecutionLimits, ExecutionPlan};
use anyhow::{Context, Result};
use std::future::Future;
use std::sync::Arc;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(crate) struct ExecutionContext {
    budget: Arc<CallBudget>,
    blocking: Arc<Semaphore>,
}

impl ExecutionContext {
    pub(crate) fn budget(&self) -> &Arc<CallBudget> {
        &self.budget
    }

    pub(crate) async fn run_blocking<F, T>(&self, work: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let permit = Arc::clone(&self.blocking)
            .acquire_owned()
            .await
            .context("Execution blocking-work scheduler is closed")?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .context("Execution blocking work panicked")?
    }
}

pub(crate) struct ExecutionScheduler {
    runtime: Runtime,
    context: ExecutionContext,
    _cancellation: Option<CancellationRegistration>,
}

impl ExecutionScheduler {
    pub(crate) fn from_plan(plan: &ExecutionPlan) -> Result<Self> {
        Self::build(&plan.body.limits, CallBudget::from_plan(plan)?, true)
    }

    #[cfg(test)]
    pub(crate) fn new(limits: &ExecutionLimits, cleanup_calls: u64) -> Result<Self> {
        Self::build(limits, CallBudget::for_tests(limits, cleanup_calls)?, false)
    }

    fn build(
        limits: &ExecutionLimits,
        budget: Arc<CallBudget>,
        register_interrupt: bool,
    ) -> Result<Self> {
        let host_parallelism = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(1);
        let planned_workers = usize::try_from(limits.max_concurrency)
            .unwrap_or(usize::MAX)
            .min(host_parallelism)
            .max(1);
        let blocking_limit = planned_workers;
        let runtime = Builder::new_multi_thread()
            .worker_threads(planned_workers)
            .max_blocking_threads(blocking_limit)
            .enable_io()
            .enable_time()
            .build()
            .context("Could not create the bounded execution scheduler")?;
        let cancellation = register_interrupt
            .then(|| register_cancellation(&budget))
            .transpose()?;
        Ok(Self {
            runtime,
            context: ExecutionContext {
                budget,
                blocking: Arc::new(Semaphore::new(blocking_limit)),
            },
            _cancellation: cancellation,
        })
    }

    pub(crate) fn run<F, Fut, T>(&self, work: F) -> Result<T>
    where
        F: FnOnce(ExecutionContext) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let context = self.context.clone();
        let result = self.runtime.block_on(work(context));
        if result.is_err() {
            self.context.budget.cancel();
        }
        result
    }

    pub(crate) fn context(&self) -> &ExecutionContext {
        &self.context
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutionScheduler;
    use crate::execution::model::sample_execution_limits;
    use anyhow::Context;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use tokio::sync::oneshot;

    #[test]
    fn scheduler_owns_one_bounded_blocking_pool() {
        let scheduler = ExecutionScheduler::new(&sample_execution_limits(), 0).expect("scheduler");
        scheduler
            .run(|context| async move {
                let active = Arc::new(AtomicU64::new(0));
                let peak = Arc::new(AtomicU64::new(0));
                let (entered_tx, entered_rx) = oneshot::channel();
                let (release_tx, release_rx) = mpsc::sync_channel(0);
                let first_context = context.clone();
                let first_active = Arc::clone(&active);
                let first_peak = Arc::clone(&peak);
                let first = tokio::spawn(async move {
                    first_context
                        .run_blocking(move || {
                            let current = first_active.fetch_add(1, Ordering::SeqCst) + 1;
                            first_peak.fetch_max(current, Ordering::SeqCst);
                            entered_tx
                                .send(())
                                .map_err(|_| anyhow::anyhow!("blocking-test observer closed"))?;
                            release_rx.recv().context("blocking-test release closed")?;
                            first_active.fetch_sub(1, Ordering::SeqCst);
                            Ok(())
                        })
                        .await
                });
                entered_rx.await.context("blocking task did not start")?;
                let second_context = context.clone();
                let second_active = Arc::clone(&active);
                let second_peak = Arc::clone(&peak);
                let second = tokio::spawn(async move {
                    second_context
                        .run_blocking(move || {
                            let current = second_active.fetch_add(1, Ordering::SeqCst) + 1;
                            second_peak.fetch_max(current, Ordering::SeqCst);
                            second_active.fetch_sub(1, Ordering::SeqCst);
                            Ok(())
                        })
                        .await
                });
                tokio::task::yield_now().await;
                assert!(
                    !second.is_finished(),
                    "blocking work must queue at the permit"
                );
                release_tx
                    .send(())
                    .context("could not release blocking task")?;
                first.await.context("first blocking task")??;
                second.await.context("second blocking task")??;
                assert_eq!(peak.load(Ordering::SeqCst), 1);
                Ok(())
            })
            .expect("scheduled execution");
        assert!(!scheduler.context().budget().is_normal_admission_closed());
    }
}
