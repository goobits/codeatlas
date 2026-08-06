use super::model::{CallCategory, CallCount, CallUsage, ExecutionLimits, ExecutionPlan};
use anyhow::Result;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

pub(crate) const CLEANUP_RESERVE_FRACTION: u64 = 5;
const MAX_CLEANUP_TIME_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallDisposition {
    Completed,
    Failed,
    Rejected,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CallRecord {
    pub sequence: u64,
    pub category: CallCategory,
    pub disposition: CallDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CallSnapshot {
    pub usage: CallUsage,
    pub records: Vec<CallRecord>,
    pub peak_concurrency: u64,
    pub peak_calls_per_second_milli: Option<u64>,
    pub termination: Option<BudgetTermination>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BudgetTermination {
    CallsExhausted,
    CleanupExhausted,
    DeadlineExhausted,
    Cancelled,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub(crate) enum CallPermitError {
    #[error("execution call budget is exhausted")]
    CallsExhausted,
    #[error("execution cleanup call allowance is exhausted")]
    CleanupExhausted,
    #[error("execution call admission was cancelled")]
    Cancelled,
    #[error("execution call deadline is exhausted")]
    DeadlineExhausted,
    #[error("execution call scheduler is closed")]
    SchedulerClosed,
}

#[derive(Clone, Copy)]
struct PendingCall {
    category: CallCategory,
    disposition: Option<CallDisposition>,
}

struct BudgetState {
    next_sequence: u64,
    normal_consumed: u64,
    cleanup_consumed: u64,
    in_flight: u64,
    peak_concurrency: u64,
    normal_next_admission: Instant,
    cleanup_next_admission: Option<Instant>,
    last_admitted_at: Option<Instant>,
    peak_calls_per_second_milli: Option<u64>,
    calls: BTreeMap<u64, PendingCall>,
    termination: Option<BudgetTermination>,
}

pub(crate) struct CallBudget {
    max_calls: u64,
    normal_limit: u64,
    cleanup_limit: u64,
    admission_interval: Duration,
    normal_deadline: Instant,
    run_deadline: Instant,
    semaphore: Arc<Semaphore>,
    admission_closed: watch::Sender<bool>,
    cancelled: watch::Sender<bool>,
    state: Mutex<BudgetState>,
}

impl CallBudget {
    pub(crate) fn from_plan(plan: &ExecutionPlan) -> Result<Arc<Self>> {
        let cleanup_calls = plan
            .body
            .expected_calls
            .iter()
            .find(|calls| calls.category == CallCategory::Cleanup)
            .map_or(0, |calls| calls.count);
        Self::new(&plan.body.limits, cleanup_calls)
    }

    #[cfg(test)]
    pub(crate) fn for_tests(limits: &ExecutionLimits, cleanup_calls: u64) -> Result<Arc<Self>> {
        Self::new(limits, cleanup_calls)
    }

    fn new(limits: &ExecutionLimits, cleanup_calls: u64) -> Result<Arc<Self>> {
        if cleanup_calls > limits.max_calls {
            anyhow::bail!(
                "Cleanup call allowance {cleanup_calls} exceeds max_calls {}",
                limits.max_calls
            );
        }
        let max_concurrency = usize::try_from(limits.max_concurrency)
            .map_err(|_| anyhow::anyhow!("max_concurrency does not fit this host"))?;
        if max_concurrency > Semaphore::MAX_PERMITS {
            anyhow::bail!("max_concurrency exceeds this host's scheduler capacity");
        }
        let started = Instant::now();
        let run_duration = Duration::from_millis(limits.run_timeout_ms);
        let cleanup_time_ms =
            (limits.run_timeout_ms / CLEANUP_RESERVE_FRACTION).clamp(1, MAX_CLEANUP_TIME_MS);
        let cleanup_duration = Duration::from_millis(cleanup_time_ms);
        let normal_duration = run_duration
            .checked_sub(cleanup_duration)
            .ok_or_else(|| anyhow::anyhow!("Execution timeout cannot reserve cleanup time"))?;
        if normal_duration.is_zero() && cleanup_calls < limits.max_calls {
            anyhow::bail!("Execution timeout leaves no time for planned non-cleanup calls");
        }
        let interval_nanos = 1_000_000_000_u64.div_ceil(limits.calls_per_second).max(1);
        let admission_interval = Duration::from_nanos(interval_nanos);
        let required_cleanup_spacing = admission_interval
            .as_nanos()
            .checked_mul(u128::from(cleanup_calls))
            .ok_or_else(|| anyhow::anyhow!("Cleanup rate reservation exceeds this host"))?;
        if cleanup_calls > 0 && required_cleanup_spacing >= cleanup_duration.as_nanos() {
            anyhow::bail!(
                "Cleanup call allowance {cleanup_calls} cannot fit the reserved cleanup time at {} calls per second",
                limits.calls_per_second
            );
        }
        let (admission_closed, _) = watch::channel(false);
        let (cancelled, _) = watch::channel(false);
        let normal_deadline = started
            .checked_add(normal_duration)
            .ok_or_else(|| anyhow::anyhow!("Normal execution deadline exceeds this host"))?;
        let run_deadline = started
            .checked_add(run_duration)
            .ok_or_else(|| anyhow::anyhow!("Execution deadline exceeds this host"))?;
        Ok(Arc::new(Self {
            max_calls: limits.max_calls,
            normal_limit: limits.max_calls - cleanup_calls,
            cleanup_limit: cleanup_calls,
            admission_interval,
            normal_deadline,
            run_deadline,
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            admission_closed,
            cancelled,
            state: Mutex::new(BudgetState {
                next_sequence: 1,
                normal_consumed: 0,
                cleanup_consumed: 0,
                in_flight: 0,
                peak_concurrency: 0,
                normal_next_admission: started,
                cleanup_next_admission: None,
                last_admitted_at: None,
                peak_calls_per_second_milli: None,
                calls: BTreeMap::new(),
                termination: None,
            }),
        }))
    }

    pub(crate) fn cancel(&self) {
        self.terminate(BudgetTermination::Cancelled);
    }

    fn stop_normal_admission(&self) {
        self.admission_closed.send_replace(true);
    }

    pub(crate) fn is_normal_admission_closed(&self) -> bool {
        *self.admission_closed.borrow()
    }

    fn has_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    pub(crate) async fn wait_for_cancellation(&self) {
        let mut cancelled = self.cancelled.subscribe();
        if *cancelled.borrow() {
            return;
        }
        let _ = cancelled.changed().await;
    }

    pub(crate) fn normal_time_remaining(&self) -> Duration {
        self.normal_deadline
            .saturating_duration_since(Instant::now())
    }

    pub(crate) fn run_time_remaining(&self) -> Duration {
        self.run_deadline.saturating_duration_since(Instant::now())
    }

    pub(crate) async fn reserve_call(
        self: &Arc<Self>,
        category: CallCategory,
    ) -> std::result::Result<CallPermit, CallPermitError> {
        let is_cleanup = category == CallCategory::Cleanup;
        let sequence = self.allocate_call(category)?;
        let deadline = if is_cleanup {
            self.run_deadline
        } else {
            self.normal_deadline
        };
        let semaphore = Arc::clone(&self.semaphore);
        let permit = match self
            .wait_for_concurrency(semaphore, deadline, is_cleanup)
            .await
        {
            Ok(permit) => permit,
            Err(error) => {
                self.record_permit_error(&error);
                self.finish_call(sequence, CallDisposition::Cancelled, false);
                return Err(error);
            }
        };
        let admission_at = self.schedule_admission(is_cleanup);
        if let Err(error) = self
            .wait_for_admission(admission_at, deadline, is_cleanup)
            .await
        {
            self.record_permit_error(&error);
            self.finish_call(sequence, CallDisposition::Cancelled, false);
            return Err(error);
        }
        {
            let mut state = self.state.lock().expect("call budget state");
            let admitted_at = Instant::now();
            state.in_flight += 1;
            state.peak_concurrency = state.peak_concurrency.max(state.in_flight);
            if let Some(rate) = state
                .last_admitted_at
                .and_then(|last| observed_rate_milli(last, admitted_at))
            {
                state.peak_calls_per_second_milli = Some(
                    state
                        .peak_calls_per_second_milli
                        .map_or(rate, |peak| peak.max(rate)),
                );
            }
            state.last_admitted_at = Some(admitted_at);
        }
        Ok(CallPermit {
            budget: Arc::clone(self),
            sequence,
            deadline,
            concurrency: Some(permit),
            finished: false,
        })
    }

    pub(crate) fn snapshot(&self) -> CallSnapshot {
        let state = self.state.lock().expect("call budget state");
        let mut counts = BTreeMap::<CallCategory, u64>::new();
        let mut records = Vec::with_capacity(state.calls.len());
        for (sequence, call) in &state.calls {
            *counts.entry(call.category).or_default() += 1;
            records.push(CallRecord {
                sequence: *sequence,
                category: call.category,
                disposition: call.disposition.unwrap_or(CallDisposition::Cancelled),
            });
        }
        let consumed = u64::try_from(records.len()).unwrap_or(u64::MAX);
        CallSnapshot {
            usage: CallUsage {
                reserved: self.max_calls,
                consumed,
                by_category: counts
                    .into_iter()
                    .map(|(category, count)| CallCount { category, count })
                    .collect(),
            },
            records,
            peak_concurrency: state.peak_concurrency,
            peak_calls_per_second_milli: state.peak_calls_per_second_milli,
            termination: state.termination,
        }
    }

    fn allocate_call(&self, category: CallCategory) -> std::result::Result<u64, CallPermitError> {
        let is_cleanup = category == CallCategory::Cleanup;
        if is_cleanup {
            self.stop_normal_admission();
        }
        let now = Instant::now();
        let deadline = if is_cleanup {
            self.run_deadline
        } else {
            self.normal_deadline
        };
        if now >= deadline {
            self.terminate(BudgetTermination::DeadlineExhausted);
            return Err(CallPermitError::DeadlineExhausted);
        }
        let mut state = self.state.lock().expect("call budget state");
        if is_cleanup {
            if state.cleanup_consumed >= self.cleanup_limit {
                drop(state);
                self.terminate(BudgetTermination::CleanupExhausted);
                return Err(CallPermitError::CleanupExhausted);
            }
            state.cleanup_consumed += 1;
        } else {
            if self.is_normal_admission_closed() {
                return Err(CallPermitError::Cancelled);
            }
            if state.normal_consumed >= self.normal_limit {
                drop(state);
                self.terminate(BudgetTermination::CallsExhausted);
                return Err(CallPermitError::CallsExhausted);
            }
            state.normal_consumed += 1;
        }
        let sequence = state.next_sequence;
        state.next_sequence += 1;
        state.calls.insert(
            sequence,
            PendingCall {
                category,
                disposition: None,
            },
        );
        Ok(sequence)
    }

    fn schedule_admission(&self, is_cleanup: bool) -> Instant {
        let now = Instant::now();
        let mut state = self.state.lock().expect("call budget state");
        let admission_at = if is_cleanup {
            let minimum = state
                .last_admitted_at
                .map(|last| last + self.admission_interval)
                .unwrap_or(now)
                .max(now);
            state.cleanup_next_admission.unwrap_or(minimum).max(minimum)
        } else {
            state.normal_next_admission.max(now)
        };
        if is_cleanup {
            state.cleanup_next_admission = Some(admission_at + self.admission_interval);
        } else {
            state.normal_next_admission = admission_at + self.admission_interval;
        }
        admission_at
    }

    async fn wait_for_admission(
        &self,
        admission_at: Instant,
        deadline: Instant,
        is_cleanup: bool,
    ) -> std::result::Result<(), CallPermitError> {
        let mut admission_closed = self.admission_closed.subscribe();
        if !is_cleanup && *admission_closed.borrow() {
            return Err(CallPermitError::Cancelled);
        }
        tokio::select! {
            _ = tokio::time::sleep_until(admission_at) => Ok(()),
            _ = tokio::time::sleep_until(deadline) => Err(CallPermitError::DeadlineExhausted),
            changed = admission_closed.changed(), if !is_cleanup => {
                match changed {
                    Ok(()) => Err(CallPermitError::Cancelled),
                    Err(_) => Err(CallPermitError::SchedulerClosed),
                }
            }
        }
    }

    async fn wait_for_concurrency(
        &self,
        semaphore: Arc<Semaphore>,
        deadline: Instant,
        is_cleanup: bool,
    ) -> std::result::Result<OwnedSemaphorePermit, CallPermitError> {
        let mut admission_closed = self.admission_closed.subscribe();
        if !is_cleanup && *admission_closed.borrow() {
            return Err(CallPermitError::Cancelled);
        }
        tokio::select! {
            permit = semaphore.acquire_owned() => permit.map_err(|_| CallPermitError::SchedulerClosed),
            _ = tokio::time::sleep_until(deadline) => Err(CallPermitError::DeadlineExhausted),
            changed = admission_closed.changed(), if !is_cleanup => {
                match changed {
                    Ok(()) => Err(CallPermitError::Cancelled),
                    Err(_) => Err(CallPermitError::SchedulerClosed),
                }
            }
        }
    }

    fn finish_call(&self, sequence: u64, disposition: CallDisposition, in_flight: bool) {
        let mut state = self.state.lock().expect("call budget state");
        if let Some(call) = state.calls.get_mut(&sequence) {
            call.disposition.get_or_insert(disposition);
        }
        if in_flight {
            state.in_flight = state.in_flight.saturating_sub(1);
        }
    }

    fn terminate(&self, termination: BudgetTermination) {
        {
            let mut state = self.state.lock().expect("call budget state");
            state.termination.get_or_insert(termination);
        }
        if termination == BudgetTermination::Cancelled {
            self.cancelled.send_replace(true);
        }
        self.stop_normal_admission();
    }

    fn record_permit_error(&self, error: &CallPermitError) {
        match error {
            CallPermitError::DeadlineExhausted => {
                self.terminate(BudgetTermination::DeadlineExhausted);
            }
            CallPermitError::SchedulerClosed => self.terminate(BudgetTermination::Cancelled),
            CallPermitError::CallsExhausted
            | CallPermitError::CleanupExhausted
            | CallPermitError::Cancelled => {}
        }
    }
}

pub(crate) struct CallPermit {
    budget: Arc<CallBudget>,
    sequence: u64,
    deadline: Instant,
    concurrency: Option<OwnedSemaphorePermit>,
    finished: bool,
}

impl CallPermit {
    #[cfg(test)]
    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(crate) fn finish(mut self, disposition: CallDisposition) {
        self.budget.finish_call(self.sequence, disposition, true);
        self.concurrency.take();
        self.finished = true;
    }
}

impl Drop for CallPermit {
    fn drop(&mut self) {
        if !self.finished {
            let disposition = if self.budget.has_cancelled() {
                CallDisposition::Cancelled
            } else {
                CallDisposition::Failed
            };
            self.budget.finish_call(self.sequence, disposition, true);
            self.concurrency.take();
        }
    }
}

fn observed_rate_milli(previous: Instant, current: Instant) -> Option<u64> {
    let duration = current.checked_duration_since(previous)?;
    if duration.is_zero() {
        return None;
    }
    let nanos = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
    Some(1_000_000_000_000_u64.div_ceil(nanos.max(1)))
}

#[cfg(test)]
mod tests {
    use super::{BudgetTermination, CallBudget, CallDisposition, CallPermitError};
    use crate::execution::artifact::sample_plan;
    use crate::execution::model::{
        sample_execution_limits, CallCategory, CallCount, ExecutionPlan,
    };
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn permits_bound_rate_concurrency_and_preserve_sequence_order() {
        let budget = CallBudget::for_tests(&sample_execution_limits(), 0).expect("budget");
        let first = budget
            .reserve_call(CallCategory::GeneratedCase)
            .await
            .expect("first permit");
        assert_eq!(first.sequence(), 1);

        let waiting_budget = budget.clone();
        let waiting =
            tokio::spawn(
                async move { waiting_budget.reserve_call(CallCategory::Validation).await },
            );
        tokio::task::yield_now().await;
        assert!(
            !waiting.is_finished(),
            "concurrency should apply backpressure"
        );
        first.finish(CallDisposition::Completed);
        tokio::time::advance(Duration::from_millis(500)).await;
        let second = waiting.await.expect("waiting task").expect("second permit");
        assert_eq!(second.sequence(), 2);
        second.finish(CallDisposition::Rejected);

        let snapshot = budget.snapshot();
        assert_eq!(snapshot.usage.reserved, 3);
        assert_eq!(snapshot.usage.consumed, 2);
        assert_eq!(snapshot.peak_concurrency, 1);
        assert!(snapshot
            .peak_calls_per_second_milli
            .is_some_and(|rate| rate <= 2_000));
        assert_eq!(
            snapshot
                .records
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(snapshot.records[1].disposition, CallDisposition::Rejected);
    }

    #[tokio::test(start_paused = true)]
    async fn normal_exhaustion_never_spends_the_cleanup_allowance() {
        let budget = CallBudget::for_tests(&sample_execution_limits(), 1).expect("budget");
        for category in [CallCategory::Setup, CallCategory::GeneratedCase] {
            let permit = budget.reserve_call(category).await.expect("normal permit");
            permit.finish(CallDisposition::Completed);
            tokio::time::advance(Duration::from_millis(500)).await;
        }
        assert!(matches!(
            budget.reserve_call(CallCategory::Retry).await,
            Err(CallPermitError::CallsExhausted)
        ));
        let cleanup = budget
            .reserve_call(CallCategory::Cleanup)
            .await
            .expect("reserved cleanup permit");
        cleanup.finish(CallDisposition::Completed);
        assert_eq!(budget.snapshot().usage.consumed, 3);
        assert_eq!(
            budget.snapshot().termination,
            Some(BudgetTermination::CallsExhausted)
        );
        assert!(matches!(
            budget.reserve_call(CallCategory::Cleanup).await,
            Err(CallPermitError::CleanupExhausted)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn concurrency_delay_cannot_release_a_rate_burst() {
        let budget = CallBudget::for_tests(&sample_execution_limits(), 0).expect("budget");
        let first = budget
            .reserve_call(CallCategory::GeneratedCase)
            .await
            .expect("first permit");

        let second_budget = budget.clone();
        let second = tokio::spawn(async move {
            second_budget
                .reserve_call(CallCategory::GeneratedCase)
                .await
        });
        tokio::task::yield_now().await;
        let third_budget = budget.clone();
        let third =
            tokio::spawn(
                async move { third_budget.reserve_call(CallCategory::GeneratedCase).await },
            );
        tokio::task::yield_now().await;

        tokio::time::advance(Duration::from_secs(1)).await;
        first.finish(CallDisposition::Completed);
        let second = second.await.expect("second task").expect("second permit");
        second.finish(CallDisposition::Completed);
        tokio::task::yield_now().await;
        assert!(
            !third.is_finished(),
            "a delayed concurrency waiter must receive a new rate slot"
        );
        tokio::time::advance(Duration::from_millis(499)).await;
        assert!(!third.is_finished());
        tokio::time::advance(Duration::from_millis(1)).await;
        third
            .await
            .expect("third task")
            .expect("third permit")
            .finish(CallDisposition::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn multiplexed_completion_order_cannot_reorder_call_evidence() {
        let mut limits = sample_execution_limits();
        limits.max_concurrency = 2;
        let budget = CallBudget::for_tests(&limits, 0).expect("budget");
        let first = budget
            .reserve_call(CallCategory::GeneratedCase)
            .await
            .expect("first permit");
        let second_budget = budget.clone();
        let second =
            tokio::spawn(async move { second_budget.reserve_call(CallCategory::Validation).await });
        tokio::time::advance(Duration::from_millis(500)).await;
        let second = second.await.expect("second task").expect("second permit");

        second.finish(CallDisposition::Rejected);
        first.finish(CallDisposition::Completed);
        let snapshot = budget.snapshot();
        assert_eq!(snapshot.peak_concurrency, 2);
        assert_eq!(snapshot.records[0].sequence, 1);
        assert_eq!(snapshot.records[0].disposition, CallDisposition::Completed);
        assert_eq!(snapshot.records[1].sequence, 2);
        assert_eq!(snapshot.records[1].disposition, CallDisposition::Rejected);
    }

    #[tokio::test(start_paused = true)]
    async fn ordinary_cleanup_closes_admission_without_marking_termination() {
        let budget = CallBudget::for_tests(&sample_execution_limits(), 1).expect("budget");
        for category in [CallCategory::GeneratedCase, CallCategory::Validation] {
            budget
                .reserve_call(category)
                .await
                .expect("normal permit")
                .finish(CallDisposition::Completed);
            tokio::time::advance(Duration::from_millis(500)).await;
        }
        let cleanup = budget
            .reserve_call(CallCategory::Cleanup)
            .await
            .expect("cleanup permit");
        cleanup.finish(CallDisposition::Completed);
        assert!(
            budget.is_normal_admission_closed(),
            "normal admission must stay closed"
        );
        assert!(matches!(
            budget.reserve_call(CallCategory::Retry).await,
            Err(CallPermitError::Cancelled)
        ));
        assert_eq!(budget.snapshot().termination, None);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_cleanup_calls_receive_distinct_rate_slots() {
        let mut limits = sample_execution_limits();
        limits.max_concurrency = 3;
        let budget = CallBudget::for_tests(&limits, 3).expect("budget");
        budget
            .reserve_call(CallCategory::Cleanup)
            .await
            .expect("first cleanup")
            .finish(CallDisposition::Completed);

        let second_budget = budget.clone();
        let second =
            tokio::spawn(async move { second_budget.reserve_call(CallCategory::Cleanup).await });
        tokio::task::yield_now().await;
        let third_budget = budget.clone();
        let third =
            tokio::spawn(async move { third_budget.reserve_call(CallCategory::Cleanup).await });
        tokio::task::yield_now().await;

        tokio::time::advance(Duration::from_millis(500)).await;
        second
            .await
            .expect("second cleanup task")
            .expect("second cleanup permit")
            .finish(CallDisposition::Completed);
        assert!(!third.is_finished(), "cleanup calls must not burst");
        tokio::time::advance(Duration::from_millis(500)).await;
        third
            .await
            .expect("third cleanup task")
            .expect("third cleanup permit")
            .finish(CallDisposition::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn plan_is_the_only_owner_of_the_cleanup_call_allowance() {
        let mut body = sample_plan().body;
        body.limits.calls_per_second = 10;
        body.expected_calls = vec![CallCount {
            category: CallCategory::Cleanup,
            count: 1,
        }];
        let plan = ExecutionPlan::new(body).expect("plan with cleanup allowance");
        let budget = CallBudget::from_plan(&plan).expect("plan budget");
        budget
            .reserve_call(CallCategory::GeneratedCase)
            .await
            .expect("one normal call")
            .finish(CallDisposition::Completed);
        assert!(matches!(
            budget.reserve_call(CallCategory::Retry).await,
            Err(CallPermitError::CallsExhausted)
        ));
        budget
            .reserve_call(CallCategory::Cleanup)
            .await
            .expect("plan-owned cleanup call")
            .finish(CallDisposition::Completed);
    }

    #[test]
    fn impossible_cleanup_rate_reservation_blocks_before_execution() {
        let mut limits = sample_execution_limits();
        limits.run_timeout_ms = 1_000;
        limits.calls_per_second = 1;
        assert!(CallBudget::for_tests(&limits, 1).is_err());
    }

    #[test]
    fn execution_blocks_when_cleanup_time_consumes_the_entire_run() {
        let mut limits = sample_execution_limits();
        limits.run_timeout_ms = 1;
        assert!(CallBudget::for_tests(&limits, 0).is_err());
    }

    #[test]
    fn execution_blocks_when_concurrency_exceeds_the_host_scheduler() {
        let mut limits = sample_execution_limits();
        limits.max_concurrency =
            u64::try_from(tokio::sync::Semaphore::MAX_PERMITS).expect("host capacity") + 1;
        limits.max_calls = limits.max_concurrency;
        assert!(CallBudget::for_tests(&limits, 0).is_err());
    }
}
