mod analysis;
mod cli;
mod commands;
mod config;
mod dead_code;
mod domain;
mod languages;
mod outputs;
mod package;
mod paths;

#[cfg(test)]
mod tests;

fn main() {
    std::process::exit(cli::run());
}
