#![deny(unreachable_pub)]

mod analysis;
mod architecture;
mod cli;
mod commands;
mod config;
mod context_slice;
mod dead_code;
mod environment;
mod execution;
mod external_tool;
mod filesystem;
mod fuzz;
mod http;
mod inspection;
mod languages;
mod lexicon;
mod outputs;
mod postgres;
#[cfg(test)]
mod published_schemas;
mod source_index;
mod testing;

#[cfg(test)]
mod tests;

fn main() {
    std::process::exit(cli::run());
}
