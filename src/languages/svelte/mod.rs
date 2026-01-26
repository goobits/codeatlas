mod parser;

use crate::domain::{Language, LanguageScanner, Route, ScanConfig, ScanReport, Symbol};
use crate::languages::scan_language;
use std::path::Path;
use walkdir::DirEntry;

pub(crate) struct SvelteScanner;

impl LanguageScanner for SvelteScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        scan_language(
            root_dir,
            config,
            Language::TypeScript, // Svelte scripts are TS/JS
            filter_entry,
            is_svelte_file,
            true, // needs source
            |path, root, source| parser::parse_file(path, root, source.unwrap_or("")),
            detect_routes,
        )
    }
}

fn filter_entry(entry: &DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    // Skip common non-source directories
    !matches!(
        name.as_ref(),
        "node_modules" | ".git" | "dist" | "build" | ".svelte-kit" | ".vercel"
    )
}

fn is_svelte_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("svelte")
    ) || is_sveltekit_script(path)
}

/// Check if this is a SvelteKit special file (+page.ts, +server.ts, etc.)
fn is_sveltekit_script(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        matches!(
            name,
            "+page.ts" | "+page.js"
                | "+page.server.ts" | "+page.server.js"
                | "+layout.ts" | "+layout.js"
                | "+layout.server.ts" | "+layout.server.js"
                | "+server.ts" | "+server.js"
                | "+error.svelte"
                | "hooks.server.ts" | "hooks.server.js"
                | "hooks.client.ts" | "hooks.client.js"
        )
    } else {
        false
    }
}

fn detect_routes(file_path: &Path, _source: &str, symbols: &mut [Symbol]) -> Vec<Route> {
    let mut routes = Vec::new();

    let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let path_str = file_path.to_string_lossy();

    // Extract route path from SvelteKit file structure
    // e.g., src/routes/api/users/+server.ts -> /api/users
    let route_path = extract_sveltekit_route(file_path);

    match file_name {
        "+server.ts" | "+server.js" => {
            // API endpoints - look for exported HTTP methods
            for sym in symbols.iter() {
                let method = match sym.name.as_str() {
                    "GET" => Some("GET"),
                    "POST" => Some("POST"),
                    "PUT" => Some("PUT"),
                    "DELETE" => Some("DELETE"),
                    "PATCH" => Some("PATCH"),
                    "OPTIONS" => Some("OPTIONS"),
                    "HEAD" => Some("HEAD"),
                    _ => None,
                };

                if let Some(method) = method {
                    routes.push(Route {
                        method: method.to_string(),
                        path: route_path.clone(),
                        handler_id: Some(sym.id.clone()),
                        source_framework: "SvelteKit".to_string(),
                        file_path: path_str.to_string(),
                        span: sym.span.clone(),
                    });
                }
            }
        }
        "+page.svelte" | "+page.ts" | "+page.js" => {
            // Page route
            routes.push(Route {
                method: "GET".to_string(),
                path: route_path,
                handler_id: None,
                source_framework: "SvelteKit".to_string(),
                file_path: path_str.to_string(),
                span: None,
            });
        }
        "+page.server.ts" | "+page.server.js" => {
            // Server-side page data loader and form actions
            for sym in symbols.iter() {
                match sym.name.as_str() {
                    "load" => {
                        routes.push(Route {
                            method: "GET".to_string(),
                            path: format!("{} (load)", route_path),
                            handler_id: Some(sym.id.clone()),
                            source_framework: "SvelteKit".to_string(),
                            file_path: path_str.to_string(),
                            span: sym.span.clone(),
                        });
                    }
                    "actions" => {
                        routes.push(Route {
                            method: "POST".to_string(),
                            path: format!("{} (actions)", route_path),
                            handler_id: Some(sym.id.clone()),
                            source_framework: "SvelteKit".to_string(),
                            file_path: path_str.to_string(),
                            span: sym.span.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }

    routes
}

/// Extract the route path from a SvelteKit file path
/// e.g., src/routes/api/users/[id]/+server.ts -> /api/users/[id]
fn extract_sveltekit_route(file_path: &Path) -> String {
    let path_str = file_path.to_string_lossy();

    // Find the routes directory
    if let Some(routes_idx) = path_str.find("/routes/") {
        let after_routes = &path_str[routes_idx + 8..]; // skip "/routes/"

        // Remove the filename
        if let Some(last_slash) = after_routes.rfind('/') {
            let route = &after_routes[..last_slash];
            if route.is_empty() {
                return "/".to_string();
            }
            // Convert (group) syntax to empty string, keep [param] syntax
            let cleaned: String = route
                .split('/')
                .filter(|seg| !seg.starts_with('(') || !seg.ends_with(')'))
                .collect::<Vec<_>>()
                .join("/");
            return format!("/{}", cleaned);
        }
    }

    "/".to_string()
}
