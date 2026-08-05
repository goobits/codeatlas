mod corpus;
mod report;

pub(crate) use report::build_inventory_with_reachability;
#[cfg(test)]
pub(crate) use report::{
    build_inventory, CodeFuzzBlockKind, CodeFuzzInventory, CODE_FUZZ_INVENTORY_SCHEMA_VERSION,
};
