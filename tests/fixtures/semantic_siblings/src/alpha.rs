use crate::Payload;

pub fn read_record(payload: Payload) -> Payload {
    payload
}

pub fn use_read_record(payload: Payload) -> Payload {
    read_record(payload)
}

pub fn write_record(path: &str, payload: Payload) -> Payload {
    let _ = std::fs::write(path, payload.value.as_bytes());
    payload
}

pub fn use_write_record(path: &str, payload: Payload) -> Payload {
    write_record(path, payload)
}

pub fn transform_record(payload: Payload) -> Payload {
    payload
}
