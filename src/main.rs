#![deny(unreachable_pub)]

mod analysis;
mod architecture;
mod cli;
mod commands;
mod config;
mod context_slice;
mod dead_code;
mod domain;
mod environment;
mod external_tool;
mod filesystem;
mod http;
mod languages;
mod lexicon;
mod outputs;
mod package;
mod paths;
mod postgres;
mod source_discovery;
mod source_index;
mod source_policy;
mod testing;

#[cfg(test)]
mod tests;

fn main() {
    std::process::exit(cli::run());
}
