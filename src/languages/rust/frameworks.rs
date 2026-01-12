use crate::domain::{Route, Symbol};

pub fn detect_routes(_symbols: &mut [Symbol]) -> Vec<Route> {
    // Axum/Actix routing is often done via method calls like `.route("/path", get(handler))`
    // This requires analyzing the main function or setup functions.
    // Similar to Python, without examining the AST of the *registration* calls (which might be in `main` or `lib`),
    // purely symbol-based detection is hard.
    
    // We will leave this empty for MVP to avoid false positives.
    Vec::new()
}
