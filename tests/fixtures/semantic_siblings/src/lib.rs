mod alpha;
mod beta;

#[derive(Clone)]
pub struct Payload {
    pub value: String,
}

pub fn exercise_fixture(payload: Payload) {
    let _ = alpha::use_read_record(payload.clone());
    let _ = beta::use_read_record(payload.clone());
    let _ = alpha::use_write_record("alpha", payload.clone());
    let _ = beta::use_write_record("beta", payload.clone());
}
