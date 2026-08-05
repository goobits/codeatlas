use super::budget::CallBudget;
use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static ACTIVE_BUDGET: Mutex<Option<Weak<CallBudget>>> = Mutex::new(None);
static INTERRUPT_HANDLER: OnceLock<std::result::Result<(), String>> = OnceLock::new();

pub(crate) struct CancellationRegistration {
    budget: Weak<CallBudget>,
}

pub(crate) fn register_cancellation(budget: &Arc<CallBudget>) -> Result<CancellationRegistration> {
    install_interrupt_handler()?;
    let mut active = ACTIVE_BUDGET
        .lock()
        .map_err(|_| anyhow::anyhow!("Execution cancellation registry is poisoned"))?;
    if active.as_ref().and_then(Weak::upgrade).is_some() {
        anyhow::bail!("Another execution already owns the process interrupt handler");
    }
    let registered = Arc::downgrade(budget);
    *active = Some(registered.clone());
    drop(active);
    if INTERRUPTED.swap(false, Ordering::SeqCst) {
        budget.cancel();
    }
    Ok(CancellationRegistration { budget: registered })
}

impl Drop for CancellationRegistration {
    fn drop(&mut self) {
        let Ok(mut active) = ACTIVE_BUDGET.lock() else {
            return;
        };
        let is_current = active
            .as_ref()
            .and_then(Weak::upgrade)
            .zip(self.budget.upgrade())
            .is_some_and(|(active, registered)| Arc::ptr_eq(&active, &registered));
        if is_current {
            *active = None;
            INTERRUPTED.store(false, Ordering::SeqCst);
        }
    }
}

fn install_interrupt_handler() -> Result<()> {
    let result = INTERRUPT_HANDLER.get_or_init(|| {
        ctrlc::set_handler(cancel_active)
            .map_err(|error| format!("Could not install the execution interrupt handler: {error}"))
    });
    result
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))
        .context("Execution cancellation is unavailable")
        .map(|_| ())
}

fn cancel_active() {
    INTERRUPTED.store(true, Ordering::SeqCst);
    let budget = ACTIVE_BUDGET
        .lock()
        .ok()
        .and_then(|active| active.as_ref().and_then(Weak::upgrade));
    if let Some(budget) = budget {
        budget.cancel();
    }
}
