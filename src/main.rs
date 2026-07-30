#![deny(unreachable_pub)]

mod analysis;
mod architecture;
mod cli;
mod commands;
mod config;
mod context_slice;
mod dead_code;
mod domain;
mod http;
mod languages;
mod outputs;
mod package;
mod paths;

#[cfg(test)]
mod tests;

fn main() {
    std::process::exit(cli::run());
}
